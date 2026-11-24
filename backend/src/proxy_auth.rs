//! Who is calling `/api/bunker/*` (#25).
//!
//! The browser never calls the bunker. It calls the frontend's Nitro server, which proxies to the
//! bunker and attaches the caller's identity, signed with HMAC-SHA256 under a secret only the two
//! hold (BANCWR_PROXY_SECRET here, NUXT_PROXY_SECRET there). The bunker trusts no identity without
//! a valid signature, so a caller reaching its port directly cannot claim to be anyone. See the
//! sign-in decision record (diogel-io/workspace 10-products/bancwr/architecture/
//! nostr-session-auth-adr.md), "Identity from the proxy to the bunker".
//!
//! An identity is a signed-in user's lowercase hex pubkey, or `service`: Nitro acting for itself
//! before anyone is signed in, which may only read the bunker's status and look a key up.
//!
//! The signature covers the request's body and a nonce as well as its method, path and identity,
//! and the bunker accepts each nonce once: a request seen in transit cannot be sent again, or sent
//! with another body, inside its time window. Nothing signed before the bunker process started
//! is accepted, so a restart, which empties the set of nonces, does not let one through again.
//!
//! frontend/tests/server/auth/proxy-signature.spec.ts asserts the same test vectors.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use nostr::hashes::{hmac, sha256, Hash, HashEngine};
use serde_json::json;

use crate::db::Database;
use crate::registry::{npub, Role};

pub const IDENTITY_HEADER: &str = "x-bancwr-identity";
pub const TIMESTAMP_HEADER: &str = "x-bancwr-timestamp";
pub const NONCE_HEADER: &str = "x-bancwr-nonce";
pub const SIGNATURE_HEADER: &str = "x-bancwr-signature";

/// The identity Nitro uses for its own calls.
pub const SERVICE_IDENTITY: &str = "service";

/// How far a signed request's timestamp may be from the bunker's clock, in seconds.
pub const MAX_SKEW_SECONDS: i64 = 30;

/// The largest body the guard reads to check the signature: axum's default limit for `Json`.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum ProxyAuthError {
    Missing,
    Malformed,
    Stale,
    BadSignature,
    Replayed,
}

impl ProxyAuthError {
    fn reason(&self) -> &'static str {
        match self {
            ProxyAuthError::Missing => "missing",
            ProxyAuthError::Malformed => "malformed",
            ProxyAuthError::Stale => "stale",
            ProxyAuthError::BadSignature => "bad_signature",
            ProxyAuthError::Replayed => "replayed",
        }
    }
}

/// What one signature covers.
pub struct Signed<'a> {
    pub timestamp: i64,
    /// 32 lowercase hex characters, new for every request.
    pub nonce: &'a str,
    pub method: &'a str,
    /// The path with its query string, exactly as sent.
    pub path: &'a str,
    pub identity: &'a str,
    /// The body's bytes, exactly as sent; empty for none.
    pub body: &'a [u8],
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Lowercase hex HMAC-SHA256 of
/// `v2\n<timestamp>\n<nonce>\n<METHOD>\n<path?query>\n<identity>\n<hex SHA-256 of the body>`.
pub fn proxy_signature(secret: &[u8], signed: &Signed) -> String {
    let canonical = format!(
        "v2\n{}\n{}\n{}\n{}\n{}\n{}",
        signed.timestamp,
        signed.nonce,
        signed.method.to_uppercase(),
        signed.path,
        signed.identity,
        hex(&sha256::Hash::hash(signed.body).to_byte_array()),
    );
    let mut engine = hmac::HmacEngine::<sha256::Hash>::new(secret);
    engine.input(canonical.as_bytes());
    hex(&hmac::Hmac::<sha256::Hash>::from_engine(engine).to_byte_array())
}

/// Compares in time independent of where the inputs first differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The proxy headers of one request, as received.
#[derive(Default)]
pub struct ProxyHeaders<'a> {
    pub identity: Option<&'a str>,
    pub timestamp: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub signature: Option<&'a str>,
}

/// A request whose signature checked out. The nonce still has to pass [`Nonces::accept`].
#[derive(Debug, PartialEq, Eq)]
pub struct Verified {
    pub identity: String,
    pub timestamp: i64,
    pub nonce: String,
}

/// Checks the signature over the request as received.
pub fn verify_proxy_request(
    secret: &[u8],
    headers: &ProxyHeaders,
    method: &str,
    path: &str,
    body: &[u8],
    now: i64,
) -> Result<Verified, ProxyAuthError> {
    let (identity, timestamp, nonce, signature) = match (headers.identity, headers.timestamp, headers.nonce, headers.signature) {
        (Some(i), Some(t), Some(n), Some(s)) => (i, t, n, s),
        _ => return Err(ProxyAuthError::Missing),
    };
    if identity != SERVICE_IDENTITY && !is_lower_hex(identity, 64) {
        return Err(ProxyAuthError::Malformed);
    }
    if !is_lower_hex(nonce, 32) {
        return Err(ProxyAuthError::Malformed);
    }
    let timestamp: i64 = timestamp.parse().map_err(|_| ProxyAuthError::Malformed)?;
    if (now - timestamp).abs() > MAX_SKEW_SECONDS {
        return Err(ProxyAuthError::Stale);
    }
    let expected = proxy_signature(secret, &Signed { timestamp, nonce, method, path, identity, body });
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(ProxyAuthError::BadSignature);
    }
    Ok(Verified { identity: identity.to_string(), timestamp, nonce: nonce.to_string() })
}

/// The nonces of signed requests still inside the time window, and when this process started.
///
/// A nonce whose timestamp has left the window is forgotten, since the window alone refuses it
/// from then on. The set lives in memory, so it starts empty in every process: a timestamp from
/// before the process started is refused as stale, because a request signed then was meant for a
/// process that no longer exists, and this one cannot know whether it was already accepted. No
/// skew is allowed on that rule. The frontend signs each request just before sending it, and the
/// two share a clock in the deployed stack, so a fresh request is never signed before the bunker
/// it reaches was serving; one signed against the process before a restart is refused, and the
/// next request the frontend signs passes.
pub struct Nonces {
    started_at: i64,
    state: Mutex<NonceState>,
}

#[derive(Default)]
struct NonceState {
    seen: HashMap<String, i64>,
    /// The second the set was last swept, so it is swept at most once a second.
    swept_at: Option<i64>,
}

impl Default for Nonces {
    /// A set for a process starting now.
    fn default() -> Self {
        Self::started_at(chrono::Utc::now().timestamp())
    }
}

impl Nonces {
    /// A set for a process that started at `started_at` (seconds since the epoch).
    pub fn started_at(started_at: i64) -> Self {
        Self { started_at, state: Mutex::new(NonceState::default()) }
    }

    /// Accepts a verified request's nonce the first time it is presented: [`ProxyAuthError::Stale`]
    /// for a timestamp from before this process started, [`ProxyAuthError::Replayed`] for a nonce
    /// already accepted.
    pub fn accept(&self, nonce: &str, timestamp: i64, now: i64) -> Result<(), ProxyAuthError> {
        if timestamp < self.started_at {
            return Err(ProxyAuthError::Stale);
        }
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.swept_at != Some(now) {
            // Keeping a nonce a second longer only refuses what the window refuses already.
            state.seen.retain(|_, t| (now - *t).abs() <= MAX_SKEW_SECONDS);
            state.swept_at = Some(now);
        }
        if state.seen.contains_key(nonce) {
            return Err(ProxyAuthError::Replayed);
        }
        state.seen.insert(nonce.to_string(), timestamp);
        Ok(())
    }
}

/// Who may call a group of routes: the backend's one access matrix (#25, #77). Administrator is
/// a superset of the other roles. `frontend/app/utils/access.ts` is the console's matching page
/// matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Bunker health: every registered role, and the service identity.
    Health,
    /// Administrators only: logs, metrics, config, bunker relays, adding and removing members.
    Administrator,
    /// Administrators and viewers: reading the team (#77).
    TeamReader,
    /// Administrators, viewers, and the service identity: the key lookup, which sign-in needs
    /// before any session exists, and a viewer's member profile page (#77).
    TeamReaderOrService,
    /// Administrators and signers, not the service identity: routes that scope what they return
    /// to the caller (#53's connection list, #142's connection tokens, #77's own recent signatures).
    Signer,
}

impl Access {
    fn allows_role(self, role: Role) -> bool {
        match self {
            Access::Health => true,
            Access::Administrator => role == Role::Administrator,
            Access::TeamReader | Access::TeamReaderOrService => matches!(role, Role::Administrator | Role::Viewer),
            Access::Signer => role.can_sign(),
        }
    }

    fn allows_service(self) -> bool {
        matches!(self, Access::Health | Access::TeamReaderOrService)
    }
}

/// The verified caller, available to handlers as `Extension<Caller>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    Service,
    Member { pubkey: String, role: Role },
}

#[derive(Clone)]
pub struct Guard {
    pub secret: Arc<[u8]>,
    pub db: Arc<Database>,
    pub access: Access,
    /// Shared by every group's guard, so a nonce used on one route is refused on all of them.
    pub nonces: Arc<Nonces>,
}

fn refuse(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}

/// Middleware for a group of `/api/bunker/*` routes:
/// 401 `not_authenticated` without a valid signature, for a nonce already used, or for a
/// timestamp from before this process started; 413
/// `payload_too_large` for a body over [`MAX_BODY_BYTES`]; 403 `not_registered` for a key not in
/// the vault; 403 `forbidden` for a role (or the service identity) the group does not allow.
/// The role is read from the vault on every request, so a removed member loses access at once.
pub async fn guard(State(guard): State<Guard>, request: Request, next: Next) -> Response {
    // The signature covers the body, so it is read here and handed on to the route afterwards.
    let (parts, body) = request.into_parts();
    let Ok(body) = axum::body::to_bytes(body, MAX_BODY_BYTES).await else {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, json!({ "error": "payload_too_large" }));
    };

    let now = chrono::Utc::now().timestamp();
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
    let headers = ProxyHeaders {
        identity: header(IDENTITY_HEADER),
        timestamp: header(TIMESTAMP_HEADER),
        nonce: header(NONCE_HEADER),
        signature: header(SIGNATURE_HEADER),
    };
    let path = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or_default();
    let verified = verify_proxy_request(&guard.secret, &headers, parts.method.as_str(), path, &body, now)
        .and_then(|v| guard.nonces.accept(&v.nonce, v.timestamp, now).map(|()| v));
    let identity = match verified {
        Ok(verified) => verified.identity,
        Err(e) => return refuse(StatusCode::UNAUTHORIZED, json!({ "error": "not_authenticated", "reason": e.reason() })),
    };

    let caller = if identity == SERVICE_IDENTITY {
        if !guard.access.allows_service() {
            return refuse(StatusCode::FORBIDDEN, json!({ "error": "forbidden" }));
        }
        Caller::Service
    } else {
        let member = match guard.db.find_member_by_pubkey(&identity) {
            Ok(member) => member,
            Err(e) => {
                tracing::error!("Failed to look up caller: {}", e);
                return refuse(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "database_error" }));
            }
        };
        let Some(member) = member else {
            return refuse(StatusCode::FORBIDDEN, json!({ "error": "not_registered", "npub": npub(&identity) }));
        };
        // A stored role outside the three (only possible before #24) grants nothing.
        match member.role() {
            Some(role) if guard.access.allows_role(role) => Caller::Member { pubkey: identity, role },
            _ => return refuse(StatusCode::FORBIDDEN, json!({ "error": "forbidden" })),
        }
    };

    let mut request = Request::from_parts(parts, Body::from(body));
    request.extensions_mut().insert(caller);
    next.run(request).await
}
