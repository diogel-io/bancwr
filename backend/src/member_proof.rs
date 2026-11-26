//! SPIKE (#151): a bunker that checks the member's own sign-in, rather than trusting the console's
//! HMAC alone to say who is calling.
//!
//! Today whoever holds `BANCWR_PROXY_SECRET` can claim any pubkey to the bunker (threat model T6),
//! so a compromised console reaches every bunker it holds a secret for, the company root's
//! included. With `BANCWR_MEMBER_PROOF=required`, a member identity is accepted only with a bunker
//! session this bunker started itself, after checking the member's NIP-98 sign-in event:
//!
//! 1. Before sign-in the console asks each bunker for a challenge (`POST /api/bunker/auth/challenge`,
//!    as the service identity). Each is single use and lives 300 s, as the console's own does.
//! 2. The member signs one NIP-98 event, as today. Its body carries the console's challenge and
//!    every bunker's (`{"challenge": …, "proofs": [ … ]}`), and its `payload` tag is that body's
//!    SHA-256, so one signature covers every bunker.
//! 3. The console forwards the `Authorization` header and the exact body to each bunker
//!    (`POST /api/bunker/auth/proof`). The bunker checks the event as the console does (kind,
//!    signature, time, `u`, `method`, `payload`), consumes its own challenge from the body, refuses
//!    its own identities (rule 10) and unregistered keys, and returns a random session token.
//! 4. The console sends that token with each request it proxies for the member
//!    (`x-bancwr-member-session`). The bunker accepts the member identity only when the token is
//!    live and was started for that same pubkey.
//!
//! What it buys: a console holding every secret still cannot invent a member. It can only use the
//! sessions of members who sign in while it is compromised. Not for merging: sessions are in
//! memory only, so restarting the bunker signs everyone out of it.

use std::collections::HashMap;
use std::sync::Mutex;

use nostr::base64::Engine as _;
use nostr::hashes::{sha256, Hash};
use nostr::{Event, JsonUtil};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::OsRng;

pub const SESSION_HEADER: &str = "x-bancwr-member-session";

pub const LOGIN_KIND: u16 = 27235;
pub const CHALLENGE_TTL_SECONDS: i64 = 300;
pub const CREATED_AT_PAST_SECONDS: i64 = 120;
pub const CREATED_AT_FUTURE_SECONDS: i64 = 30;
/// As the console's own session: 12 hours.
pub const SESSION_SECONDS: i64 = 12 * 60 * 60;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn random_hex() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256::Hash::hash(data).to_byte_array())
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProofError {
    Malformed,
    WrongKind,
    BadSignature,
    Stale,
    WrongUrl,
    WrongMethod,
    PayloadMismatch,
    /// No challenge in the body that this bunker issued and has not yet seen used.
    UnknownChallenge,
}

impl ProofError {
    pub fn reason(&self) -> &'static str {
        match self {
            ProofError::Malformed => "malformed_authorization",
            ProofError::WrongKind => "wrong_kind",
            ProofError::BadSignature => "bad_signature",
            ProofError::Stale => "stale_event",
            ProofError::WrongUrl => "wrong_url",
            ProofError::WrongMethod => "wrong_method",
            ProofError::PayloadMismatch => "payload_mismatch",
            ProofError::UnknownChallenge => "unknown_challenge",
        }
    }
}

pub struct MemberProof {
    /// BANCWR_CONSOLE_URL: the `u` tag must be this plus `/api/auth/login`.
    login_url: String,
    challenges: Mutex<HashMap<String, i64>>,
    /// By the SHA-256 of the token, never the token: pubkey and expiry.
    sessions: Mutex<HashMap<String, (String, i64)>>,
}

impl MemberProof {
    pub fn new(console_url: &str) -> Self {
        Self {
            login_url: format!("{}/api/auth/login", console_url.trim_end_matches('/')),
            challenges: Mutex::default(),
            sessions: Mutex::default(),
        }
    }

    pub fn issue_challenge(&self, now: i64) -> String {
        let challenge = random_hex();
        let mut challenges = self.challenges.lock().unwrap_or_else(|p| p.into_inner());
        challenges.retain(|_, expires| *expires >= now);
        challenges.insert(challenge.clone(), now + CHALLENGE_TTL_SECONDS);
        challenge
    }

    /// The pubkey that signed this sign-in, once every rule passes. Any challenge of this bunker's
    /// in the body is consumed whether or not the rest passes, as the console's rule 1 does.
    pub fn verify(&self, authorization: &str, body: &str, now: i64) -> Result<String, ProofError> {
        let proofs: Vec<String> = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("proofs").and_then(|p| p.as_array()).cloned())
            .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let challenge_ok = {
            let mut challenges = self.challenges.lock().unwrap_or_else(|p| p.into_inner());
            let mut ok = false;
            for proof in &proofs {
                if let Some(expires) = challenges.remove(proof) {
                    ok |= expires >= now;
                }
            }
            ok
        };

        let encoded = authorization.strip_prefix("Nostr ").ok_or(ProofError::Malformed)?;
        let json = nostr::base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ProofError::Malformed)?;
        let event = Event::from_json(json).map_err(|_| ProofError::Malformed)?;
        if event.kind.as_u16() != LOGIN_KIND {
            return Err(ProofError::WrongKind);
        }
        event.verify().map_err(|_| ProofError::BadSignature)?;
        let created_at = event.created_at.as_u64() as i64;
        if created_at < now - CREATED_AT_PAST_SECONDS || created_at > now + CREATED_AT_FUTURE_SECONDS {
            return Err(ProofError::Stale);
        }
        let single = |name: &str| -> Option<String> {
            let mut values = event.tags.iter().map(|t| t.as_slice()).filter(|t| t.first().map(String::as_str) == Some(name));
            let first = values.next()?;
            if values.next().is_some() {
                return None;
            }
            first.get(1).cloned()
        };
        if single("u").as_deref() != Some(self.login_url.as_str()) {
            return Err(ProofError::WrongUrl);
        }
        if single("method").as_deref() != Some("POST") {
            return Err(ProofError::WrongMethod);
        }
        if single("payload").as_deref() != Some(sha256_hex(body.as_bytes()).as_str()) {
            return Err(ProofError::PayloadMismatch);
        }
        if !challenge_ok {
            return Err(ProofError::UnknownChallenge);
        }
        Ok(event.pubkey.to_hex())
    }

    pub fn start_session(&self, pubkey: &str, now: i64) -> (String, i64) {
        let token = random_hex();
        let expires = now + SESSION_SECONDS;
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.retain(|_, (_, e)| *e >= now);
        sessions.insert(sha256_hex(token.as_bytes()), (pubkey.to_string(), expires));
        (token, expires)
    }

    /// The pubkey a live session token was started for.
    pub fn session_pubkey(&self, token: &str, now: i64) -> Option<String> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        match sessions.get(&sha256_hex(token.as_bytes())) {
            Some((pubkey, expires)) if *expires >= now => Some(pubkey.clone()),
            _ => None,
        }
    }

    pub fn end_session(&self, token: &str) {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&sha256_hex(token.as_bytes()));
    }
}
