//! NIP-46 remote signing (#53), read at nips commit 0046368a.
//!
//! A client may use the bunker only after `connect` with the secret of a single-use connection
//! token an administrator issued for a vault member. The connection is stored (it survives a
//! restart), attributed to that member, and allowed exactly the permissions granted: the token's,
//! intersected with what the client asked for. Every request after `connect` needs an active
//! connection whose member is still in the vault and may sign: an administrator or a signer (#77,
//! #143). A member whose stored role is not recognised is refused as a removed member is. Clients sign as the
//! bunker's key: "for a member" is attribution and accountability, not a separate key.
//!
//! The encryption methods (#97) are the same: each is a permission of its own, named as NIP-46
//! names it, and the bunker encrypts and decrypts with its own key. Every decrypt is recorded as an
//! audit event before it happens. A peer that is a NIP-46 app's key is refused, because the
//! traffic between apps and the bunker is encrypted with that same key: any key that has sent the
//! bunker a NIP-46 request counts, whether or not it ever connected.
//!
//! A `connect` whose secret matches a live token but is refused (for its identity, its member or
//! the permissions it asks for) uses the token up: it is never redeemed afterwards.
//!
//! Each connection is rate limited (#101, `rate_limit.rs`): the methods that use the key for the
//! app count, checked once the connection is found and before anything else, so a refused request
//! signs, decrypts and records nothing.
//!
//! A connection's kinds may need an administrator's approval (#102, `approvals.rs`): a
//! `sign_event` for one is held rather than signed, answered at once with an auth challenge, and
//! answered for real once it is approved, rejected, timed out or its connection ends.
use crate::approvals::{self, ApprovalQueue, Pending, QueueFull, Scheme};
use crate::audit::action;
use crate::db::{ClientMetadata, Database, Nip46Connection, RedeemRefusal};
use crate::rate_limit::{self, Decision, RateLimiter};
use crate::keyring::Keyring;
use crate::signer::Signer;
use chrono::Utc;
use nostr::hashes::{sha256, Hash};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use uuid::Uuid;
use tracing::{info, warn};

/// NIP-46 JSON-RPC request
#[derive(Debug, Deserialize, Serialize)]
pub struct Nip46Request {
    pub id: String,
    pub method: String,
    pub params: Vec<String>,
}

/// NIP-46 JSON-RPC response
#[derive(Debug, Serialize, Deserialize)]
pub struct Nip46Response {
    pub id: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl Nip46Response {
    /// The NIP-46 auth challenge: the app shows `url` and keeps waiting for the same request id.
    pub fn auth_url(id: String, url: impl Into<String>) -> Self {
        Self { id, result: Some("auth_url".to_string()), error: Some(url.into()) }
    }

    fn ok(id: String, result: impl Into<String>) -> Self {
        Self { id, result: Some(result.into()), error: None }
    }

    fn err(id: String, error: impl Into<String>) -> Self {
        Self { id, result: None, error: Some(error.into()) }
    }
}

/// The hex SHA-256 of a connection secret, which is all the database keeps of it.
pub fn hash_secret(secret: &str) -> String {
    sha256::Hash::hash(secret.as_bytes()).to_string()
}

/// The permission a `sign_event` of `kind` needs.
pub fn sign_permission(kind: u16) -> String {
    format!("sign_event:{}", kind)
}

/// The NIP-46 encryption methods (#97). Each is its own permission, named as the method is: NIP-46
/// writes permissions as `method[:params]`, and these take no parameter.
pub const ENCRYPTION_METHODS: [&str; 4] = ["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"];

/// Shown to a client that has not connected, or whose connection ended.
pub const NOT_CONNECTED: &str = "Not connected: connect with a connection token first";

/// Shown when a connection's identity is not unlocked in this bunker (#103): it was added after
/// the bunker started, which a restart fixes.
pub const IDENTITY_UNAVAILABLE: &str = "This identity is not available in this bunker yet: it loads when the bunker restarts";

/// The refusal of a request from a new key once `NEW_NIP46_SENDERS_PER_HOUR` were recorded.
pub const TOO_MANY_NEW_SENDERS: &str = "Too many new apps this hour: try again later";
/// Added to a `connect` refusal that used up its token (`RedeemRefusal::uses_up_token`).
pub const TOKEN_USED_UP: &str = ". The token is now used up: ask an administrator for a new one";

/// Shown once to a client whose connection has just reached its expiry (#98). After that it is a
/// revoked connection like any other, and gets `NOT_CONNECTED`.
pub const EXPIRED: &str = "This connection has expired";

/// What `Nip46Handler::handle` decided about a request.
#[derive(Debug)]
pub enum Nip46Outcome {
    /// Answer it now.
    Reply(Nip46Response),
    /// Held for approval (#102): send `notice`, the auth challenge, when there is one. The real
    /// answer comes later, as a `Reply`.
    Held { pending_id: Uuid, notice: Option<Nip46Response> },
}

/// An answer to send later (#102), to `client_pubkey`, encrypted as its request was.
#[derive(Debug)]
pub struct LaterReply {
    pub client_pubkey: PublicKey,
    pub scheme: Scheme,
    /// The identity the request was addressed to (#103), which answers it. None: the primary.
    pub identity: Option<String>,
    pub response: Nip46Response,
}

impl LaterReply {
    fn to(pending: &Pending, response: Nip46Response) -> Self {
        Self { client_pubkey: pending.client_pubkey, scheme: pending.scheme, identity: pending.connection.identity_pubkey.clone(), response }
    }
}

/// Why `Nip46Handler::approve` did not sign.
#[derive(Debug)]
pub enum ApprovalError {
    /// No such request is waiting: unknown, or already decided.
    NotFound,
    /// The administrator approved another event than the one waiting.
    EventMismatch,
    /// The connection ended, or may no longer sign this kind: the request is cancelled, and this is
    /// the app's answer.
    ConnectionEnded(Box<LaterReply>),
    /// It could not be recorded, so nothing was signed and the request still waits.
    Internal,
}

/// The error a client over its rate limit gets (#101).
pub fn rate_limited_message(limit: rate_limit::RateLimit, retry_after_secs: u64) -> String {
    format!(
        "Rate limited: this connection may make {} requests a minute (burst {}). Try again in {} s",
        limit.per_minute, limit.burst, retry_after_secs
    )
}

/// Marks every connection past its expiry (#98) as `expired`, and logs each. Returns those it
/// ended. A failure is logged and ends nothing: expired connections are refused regardless, since
/// `active_nip46_connection` never returns one. Called by the handler, at start and hourly
/// (`main.rs`), and before the connections are listed.
pub fn expire_connections(db: &Database) -> Vec<Nip46Connection> {
    match db.expire_nip46_connections(Utc::now()) {
        Ok(ended) => {
            for connection in &ended {
                warn!("NIP-46 connection {} for {} from {} expired", connection.id, connection.for_pubkey, connection.client_pubkey);
            }
            ended
        }
        Err(e) => {
            warn!("Failed to mark expired NIP-46 connections: {}", e);
            Vec::new()
        }
    }
}

/// NIP-46 protocol handler
pub struct Nip46Handler {
    /// Every identity this bunker signs as (#103): the primary, and any added beside it.
    keyring: Arc<Keyring>,
    db: Database,
    limiter: RateLimiter,
    /// Requests refused for the rate limit since the bunker started (#101), for the metrics.
    rate_limited: AtomicU64,
    /// Requests held for approval (#102).
    approvals: ApprovalQueue,
    approval_settings: Mutex<ApprovalSettings>,
}

/// How long a held request waits, and where the console is, for its auth challenge (#102).
#[derive(Debug, Clone)]
struct ApprovalSettings {
    timeout: chrono::Duration,
    console_url: Option<String>,
}

impl Nip46Handler {
    /// A handler for a bunker with one identity, `signer`.
    pub fn new(signer: Arc<RwLock<Signer>>, db: Database) -> Self {
        Self::with_keyring(Arc::new(Keyring::new(signer, Vec::new())), db)
    }

    /// A handler signing as every identity in `keyring` (#103).
    pub fn with_keyring(keyring: Arc<Keyring>, db: Database) -> Self {
        Self {
            keyring,
            db,
            limiter: RateLimiter::new(),
            rate_limited: AtomicU64::new(0),
            approvals: ApprovalQueue::new(),
            approval_settings: Mutex::new(ApprovalSettings {
                timeout: chrono::Duration::minutes(crate::config::DEFAULT_APPROVAL_TIMEOUT_MINUTES.into()),
                console_url: None,
            }),
        }
    }

    /// Sets how long a held request waits, and the console's origin for its auth challenge
    /// (#102). `AppState::new` sets them from the config; tests shorten the timeout.
    pub fn configure_approvals(&self, timeout: chrono::Duration, console_url: Option<String>) {
        *self.approval_settings.lock().unwrap_or_else(|p| p.into_inner()) = ApprovalSettings { timeout, console_url };
    }

    fn approval_settings(&self) -> ApprovalSettings {
        self.approval_settings.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Requests waiting for approval, for /api/bunker/metrics.
    pub fn approvals_pending(&self) -> usize {
        self.approvals.len()
    }

    /// Requests waiting for approval, oldest first: all, or those for `for_pubkey`.
    pub fn pending_approvals(&self, for_pubkey: Option<&str>) -> Vec<Pending> {
        self.approvals.list(for_pubkey)
    }

    /// Requests refused for the rate limit since the bunker started (#101).
    pub fn rate_limited_count(&self) -> u64 {
        self.rate_limited.load(Ordering::Relaxed)
    }

    /// Drops the rate limiter's idle buckets (#101). Called hourly (`main.rs`).
    pub fn sweep_rate_limits(&self) -> usize {
        self.limiter.sweep()
    }

    /// Refuses `request` when the connection is over its rate limit (#101), counting the refusal
    /// and, at most once a minute per connection, logging and auditing it. A failure to audit is
    /// logged and still refuses.
    fn rate_limit_refusal(&self, request: &Nip46Request, connection: &Nip46Connection) -> Option<Nip46Response> {
        if !rate_limit::counts(&request.method) {
            return None;
        }
        let limit = connection.effective_rate_limit();
        let Decision::Limited { retry_after_secs, report } = self.limiter.check(&connection.id, limit) else {
            return None;
        };
        self.rate_limited.fetch_add(1, Ordering::Relaxed);
        if report {
            warn!(
                "NIP-46 connection {} for {} from {} is over its rate limit ({} a minute, burst {}); refusing until it slows down",
                connection.id, connection.for_pubkey, connection.client_pubkey, limit.per_minute, limit.burst
            );
            if let Err(e) = self.db.record_rate_limited(connection, limit, Utc::now()) {
                warn!("Failed to record the rate limiting of NIP-46 connection {}: {}", connection.id, e);
            }
        }
        Some(Nip46Response::err(request.id.clone(), rate_limited_message(limit, retry_after_secs)))
    }

    /// Active connections, for /api/bunker/metrics.
    pub async fn connection_count(&self) -> usize {
        self.db.active_nip46_connection_count().unwrap_or_else(|e| {
            warn!("Failed to count NIP-46 connections: {}", e);
            0
        })
    }

    /// Handles a request that arrived encrypted with NIP-44, and returns what to answer now: for a
    /// request held for approval (#102), its auth challenge (whether or not the relay would send
    /// one). For tests, and callers that never hold; the relay client uses `handle`.
    pub async fn handle_request(&self, request: Nip46Request, client_pubkey: PublicKey) -> Nip46Response {
        let id = request.id.clone();
        match self.handle(request, client_pubkey, Scheme::Nip44).await {
            Nip46Outcome::Reply(response) => response,
            Nip46Outcome::Held { pending_id, notice } => notice.unwrap_or_else(|| Nip46Response::auth_url(id, format!("/approvals/{}", pending_id))),
        }
    }

    /// Handles a request addressed to the primary identity.
    pub async fn handle(&self, request: Nip46Request, client_pubkey: PublicKey, scheme: Scheme) -> Nip46Outcome {
        self.handle_for(request, client_pubkey, scheme, None).await
    }

    /// Handles a request addressed to `identity` (#103; None: the primary), as the relay client
    /// read it from the request's `p` tag. A connection answers only for its own identity.
    pub async fn handle_for(&self, request: Nip46Request, client_pubkey: PublicKey, scheme: Scheme, identity: Option<String>) -> Nip46Outcome {
        info!("Handling NIP-46 request: method={}, id={}", request.method, request.id);
        // Every sender is recorded before its request is handled, connect included, so the
        // encryption methods refuse its key as a peer whatever becomes of the request. If that
        // cannot be written, the request is refused.
        match self.db.record_nip46_sender(&client_pubkey.to_hex(), Utc::now()) {
            Ok(true) => {}
            Ok(false) => {
                warn!("Too many new NIP-46 senders this hour; refused a request from {}", client_pubkey);
                return Nip46Outcome::Reply(Nip46Response::err(request.id, TOO_MANY_NEW_SENDERS));
            }
            Err(e) => {
                warn!("Failed to record the sender {} of a NIP-46 request, so it is refused: {}", client_pubkey, e);
                return Nip46Outcome::Reply(Nip46Response::err(request.id, "Internal error"));
            }
        }
        if request.method == "connect" {
            // An app reconnecting after its connection expired (#98): mark that one `expired`
            // first, rather than `replaced`.
            expire_connections(&self.db);
            return Nip46Outcome::Reply(self.handle_connect(request, client_pubkey, identity.as_deref()).await);
        }

        let connection = match self.db.active_nip46_connection(&client_pubkey.to_hex()) {
            Ok(Some(connection)) => connection,
            Ok(None) => {
                // Perhaps it has just expired (#98): it is refused either way, but the first
                // refusal ends it and says why.
                let client = client_pubkey.to_hex();
                let expired = expire_connections(&self.db).into_iter().any(|c| c.client_pubkey == client);
                return Nip46Outcome::Reply(Nip46Response::err(request.id, if expired { EXPIRED } else { NOT_CONNECTED }));
            }
            Err(e) => {
                warn!("Failed to read the NIP-46 connection of {}: {}", client_pubkey, e);
                return Nip46Outcome::Reply(Nip46Response::err(request.id, "Internal error"));
            }
        };

        // An app connected to one identity is not connected to another (#103).
        if connection.identity_pubkey != identity {
            return Nip46Outcome::Reply(Nip46Response::err(request.id, NOT_CONNECTED));
        }

        if let Some(refusal) = self.rate_limit_refusal(&request, &connection) {
            return Nip46Outcome::Reply(refusal);
        }

        if request.method == "sign_event" {
            return self.handle_sign_event(request, client_pubkey, &connection, scheme).await;
        }
        Nip46Outcome::Reply(match request.method.as_str() {
            "get_public_key" => match self.keyring.signer(connection.identity_pubkey.as_deref()).await {
                Some(signer) => Nip46Response::ok(request.id, signer.public_key_hex()),
                None => Nip46Response::err(request.id, IDENTITY_UNAVAILABLE),
            },
            "ping" => Nip46Response::ok(request.id, "pong"),
            // No relay change is proposed. An administrator can change the bunker's relays (#78),
            // but moving connected apps to the new list is out of its scope: an app on a removed
            // relay reconnects with a new bunker:// string.
            "switch_relays" => Nip46Response::ok(request.id, "null"),
            "logout" => {
                // NIP-46: reply "ack", and remove the session. The reply is built first; the
                // revocation cannot fail in a way the client could act on.
                if let Err(e) = self.db.revoke_nip46_connection(&connection.id, "logout", Utc::now()) {
                    warn!("Failed to revoke the NIP-46 connection {} on logout: {}", connection.id, e);
                }
                Nip46Response::ok(request.id, "ack")
            }
            "nip04_encrypt" | "nip04_decrypt" | "nip44_encrypt" | "nip44_decrypt" => self.handle_crypto(request, &connection).await,
            _ => Nip46Response::err(request.id, format!("Unknown method: {}", request.method)),
        })
    }

    /// `connect`: `[remote-signer-pubkey, secret, requested-perms?, client-metadata?]`.
    async fn handle_connect(&self, request: Nip46Request, client_pubkey: PublicKey, identity: Option<&str>) -> Nip46Response {
        let Some(bunker) = self.keyring.signer(identity).await.map(|signer| signer.public_key_hex()) else {
            return Nip46Response::err(request.id, IDENTITY_UNAVAILABLE);
        };
        if request.params.first().map(|p| p.trim().to_lowercase()) != Some(bunker) {
            return Nip46Response::err(request.id, "This connect is not addressed to this bunker");
        }
        let secret = request.params.get(1).map(|s| s.trim()).unwrap_or("");
        if secret.is_empty() {
            return Nip46Response::err(request.id, "A connection token's secret is required");
        }
        let requested = request.params.get(2).map(String::as_str);
        let metadata = request.params.get(3).map(|m| parse_metadata(m)).unwrap_or_default();

        match self.db.redeem_nip46_token_for(&hash_secret(secret), identity, &client_pubkey.to_hex(), requested, &metadata, Utc::now()) {
            Ok(Ok(connection)) => {
                info!("NIP-46 client {} connected for {} with {}", client_pubkey, connection.for_pubkey, connection.perms.join(","));
                Nip46Response::ok(request.id, "ack")
            }
            Ok(Err(refusal)) => {
                warn!("NIP-46 connect from {} refused: {:?}", client_pubkey, refusal);
                let reason = match refusal {
                    RedeemRefusal::UnknownSecret => "Invalid secret",
                    RedeemRefusal::Used => "This connection token has already been used",
                    RedeemRefusal::Revoked => "This connection token has been revoked",
                    RedeemRefusal::Expired => "This connection token has expired",
                    RedeemRefusal::MemberRemoved => "This connection token's member is no longer in the vault",
                    RedeemRefusal::MemberCannotSign => "This connection token's member has no recognised role and cannot sign",
                    RedeemRefusal::NothingGrantable => "None of the requested permissions are allowed by this connection token",
                    RedeemRefusal::NoIdentityAccess => "This connection token's member may no longer sign as this identity",
                    RedeemRefusal::OtherIdentity => "This connection token is for another identity",
                };
                if refusal.uses_up_token() {
                    Nip46Response::err(request.id, format!("{}{}", reason, TOKEN_USED_UP))
                } else {
                    Nip46Response::err(request.id, reason)
                }
            }
            Err(e) => {
                warn!("Failed to redeem a NIP-46 connection token from {}: {}", client_pubkey, e);
                Nip46Response::err(request.id, "Internal error")
            }
        }
    }

    /// sign_event: only kinds the connection was granted, and those that need approval (#102) only
    /// once an administrator approves.
    async fn handle_sign_event(&self, request: Nip46Request, client_pubkey: PublicKey, connection: &Nip46Connection, scheme: Scheme) -> Nip46Outcome {
        let unsigned_event = match self.event_to_sign(&request, connection).await {
            Ok(event) => event,
            Err(response) => return Nip46Outcome::Reply(response),
        };
        let kind = unsigned_event.kind.as_u16();
        if !connection.perms.contains(&sign_permission(kind)) {
            return Nip46Outcome::Reply(Nip46Response::err(request.id, format!("Forbidden: this connection may not sign kind {}", kind)));
        }
        if connection.needs_approval(kind) {
            return self.hold(request, client_pubkey, connection, scheme, unsigned_event);
        }
        Nip46Outcome::Reply(self.sign_now(request.id, client_pubkey, connection, unsigned_event).await)
    }

    /// The event a `sign_event` asks for, as the bunker would sign it: its own key, with the id
    /// computed. Or the answer refusing it.
    async fn event_to_sign(&self, request: &Nip46Request, connection: &Nip46Connection) -> Result<UnsignedEvent, Nip46Response> {
        let Some(unsigned_event_json) = request.params.first() else {
            return Err(Nip46Response::err(request.id.clone(), "Missing event to sign"));
        };
        // NIP-46 defines the parameter as `{kind, content, tags, created_at}`, with no pubkey, and
        // nostr-tools sends exactly that; some clients add a pubkey. Either way the event is signed
        // as the bunker's key, so the bunker's key is what goes in (#31 found the strict parse).
        let mut template: serde_json::Value = match serde_json::from_str(unsigned_event_json) {
            Ok(value @ serde_json::Value::Object(_)) => value,
            Ok(_) => return Err(Nip46Response::err(request.id.clone(), "Invalid event JSON: expected an object")),
            Err(e) => return Err(Nip46Response::err(request.id.clone(), format!("Invalid event JSON: {}", e))),
        };
        let Some(identity) = self.keyring.signer(connection.identity_pubkey.as_deref()).await.map(|signer| signer.public_key_hex()) else {
            return Err(Nip46Response::err(request.id.clone(), IDENTITY_UNAVAILABLE));
        };
        template["pubkey"] = serde_json::Value::String(identity);
        if let Some(object) = template.as_object_mut() {
            // Any id the client computed was for its own idea of the event; it is recomputed.
            object.remove("id");
        }
        let mut unsigned_event: UnsignedEvent = match serde_json::from_value(template) {
            Ok(ev) => ev,
            Err(e) => return Err(Nip46Response::err(request.id.clone(), format!("Invalid event JSON: {}", e))),
        };
        unsigned_event.id = None;
        unsigned_event.ensure_id();
        Ok(unsigned_event)
    }

    /// Parks a `sign_event` for approval (#102), recording it first: if that fails, or the queue
    /// is full, it is refused and nothing waits.
    fn hold(&self, request: Nip46Request, client_pubkey: PublicKey, connection: &Nip46Connection, scheme: Scheme, event: UnsignedEvent) -> Nip46Outcome {
        let settings = self.approval_settings();
        let now = Utc::now();
        let Some(event_id) = event.id else {
            return Nip46Outcome::Reply(Nip46Response::err(request.id, "Internal error"));
        };
        let pending = Pending {
            id: Uuid::new_v4(),
            request_id: request.id.clone(),
            client_pubkey,
            scheme,
            connection: connection.clone(),
            event,
            event_id,
            requested_at: now,
            expires_at: now + settings.timeout,
        };
        let (pending_id, detail) = (pending.id, pending.audit_detail());
        if let Err(full) = self.approvals.hold(pending) {
            warn!("NIP-46 connection {} asked to sign kind {}, but {} too many requests are waiting", connection.id, event_kind(&detail), match full {
                QueueFull::Connection => "it has",
                QueueFull::Bunker => "the bunker has",
            });
            return Nip46Outcome::Reply(Nip46Response::err(request.id, approvals::QUEUE_FULL));
        }
        if let Err(e) = self.db.record_approval(action::APPROVAL_REQUESTED, connection, None, detail, now) {
            warn!("Failed to record a request for approval from {}, so it is refused: {}", connection.client_pubkey, e);
            self.approvals.take(pending_id);
            return Nip46Outcome::Reply(Nip46Response::err(request.id, "Internal error"));
        }
        info!("NIP-46 connection {} for {} asked to sign; held for approval as {}", connection.id, connection.for_pubkey, pending_id);
        let notice = settings.console_url.map(|console| Nip46Response::auth_url(request.id, format!("{}/approvals/{}", console, pending_id)));
        Nip46Outcome::Held { pending_id, notice }
    }

    /// Signs `unsigned_event` for the connection now, logging it as a signature.
    async fn sign_now(&self, request_id: String, client_pubkey: PublicKey, connection: &Nip46Connection, unsigned_event: UnsignedEvent) -> Nip46Response {
        let kind = unsigned_event.kind.as_u16();
        let Some(signer) = self.keyring.signer(connection.identity_pubkey.as_deref()).await else {
            return Nip46Response::err(request_id, IDENTITY_UNAVAILABLE);
        };
        let signed = signer.sign_event(unsigned_event).await;
        drop(signer);
        match signed {
            Ok(signed_event) => {
                let now = Utc::now();
                if let Err(e) = self.db.touch_nip46_connection(&connection.id, now) {
                    warn!("Failed to record NIP-46 connection use: {}", e);
                }
                // The client asked; the member is who the connection signs for (diogel-io/workspace#38).
                if let Err(e) = self.db.log_signing_event_for(
                    &signed_event.id.to_hex(),
                    &client_pubkey.to_hex(),
                    Some(&connection.for_pubkey),
                    Some(&connection.id),
                    connection.identity_pubkey.as_deref(),
                    kind as u32,
                    now,
                ) {
                    warn!("Failed to log signing event: {}", e);
                }
                Nip46Response::ok(request_id, signed_event.as_json())
            }
            Err(e) => Nip46Response::err(request_id, format!("Signing error: {}", e)),
        }
    }
}

/// The kind in an approval's audit detail, for a log line.
fn event_kind(detail: &str) -> &str {
    detail.split(';').find_map(|part| part.strip_prefix("kind=")).unwrap_or("?")
}

impl Nip46Handler {
    /// Approves the waiting request `id` (#102), if `event_id` is the event waiting: the connection
    /// is checked again, then the event is signed, logged and recorded as `approval_approved` by
    /// `actor`. Returns the app's answer, to send.
    pub async fn approve(&self, id: Uuid, event_id: &str, actor: Option<&str>, note: Option<&str>) -> Result<LaterReply, ApprovalError> {
        let waiting = self.approvals.get(id).ok_or(ApprovalError::NotFound)?;
        if !waiting.event_id.to_hex().eq_ignore_ascii_case(event_id.trim()) {
            return Err(ApprovalError::EventMismatch);
        }
        let pending = self.approvals.take(id).ok_or(ApprovalError::NotFound)?;
        let now = Utc::now();
        let note = approvals::clean_note(note);

        // Still active, still for a member who may sign, and still allowed this kind.
        let current = match self.db.active_nip46_connection(&pending.client_pubkey.to_hex()) {
            Ok(current) => current.filter(|c| c.id == pending.connection.id && c.perms.contains(&sign_permission(pending.kind()))),
            Err(e) => {
                warn!("Failed to read the NIP-46 connection of {} to approve {}: {}", pending.client_pubkey, id, e);
                self.put_back(pending);
                return Err(ApprovalError::Internal);
            }
        };
        let Some(connection) = current else {
            return Err(ApprovalError::ConnectionEnded(Box::new(self.cancel(pending, now))));
        };

        let mut detail = pending.audit_detail();
        if let Some(note) = &note {
            detail.push_str(&format!(";note={}", note));
        }
        if let Err(e) = self.db.record_approval(action::APPROVAL_APPROVED, &connection, actor, detail, now) {
            warn!("Failed to record the approval of {}, so nothing was signed: {}", id, e);
            self.put_back(pending);
            return Err(ApprovalError::Internal);
        }
        info!("Approval {} for NIP-46 connection {} approved; signing kind {}", id, connection.id, pending.kind());
        let response = self.sign_now(pending.request_id.clone(), pending.client_pubkey, &connection, pending.event.clone()).await;
        Ok(LaterReply::to(&pending, response))
    }

    /// Rejects the waiting request `id` (#102), recorded as `approval_rejected` by `actor`. Returns
    /// the app's answer, to send, or None when no such request is waiting.
    pub fn reject(&self, id: Uuid, actor: Option<&str>, note: Option<&str>) -> Result<LaterReply, ApprovalError> {
        let pending = self.approvals.take(id).ok_or(ApprovalError::NotFound)?;
        let now = Utc::now();
        let note = approvals::clean_note(note);
        let mut detail = pending.audit_detail();
        if let Some(note) = &note {
            detail.push_str(&format!(";note={}", note));
        }
        if let Err(e) = self.db.record_approval(action::APPROVAL_REJECTED, &pending.connection, actor, detail, now) {
            warn!("Failed to record the rejection of {}: {}", id, e);
            self.put_back(pending);
            return Err(ApprovalError::Internal);
        }
        info!("Approval {} for NIP-46 connection {} rejected", id, pending.connection.id);
        let response = Nip46Response::err(pending.request_id.clone(), approvals::rejected_message(note.as_deref()));
        Ok(LaterReply::to(&pending, response))
    }

    /// Ends every request past its expiry, and every request whose connection has ended (#102).
    /// Each is recorded, and the answers are returned, to send. Called every few seconds
    /// (`main.rs`).
    pub fn sweep_approvals(&self, now: chrono::DateTime<Utc>) -> Vec<LaterReply> {
        let timeout = self.approval_settings().timeout;
        let mut replies = Vec::new();
        for pending in self.approvals.take_expired(now) {
            let detail = format!("{};timeout_seconds={}", pending.audit_detail(), timeout.num_seconds());
            if let Err(e) = self.db.record_approval(action::APPROVAL_EXPIRED, &pending.connection, None, detail, now) {
                warn!("Failed to record that approval {} timed out: {}", pending.id, e);
            }
            warn!("Approval {} for NIP-46 connection {} timed out; nothing was signed", pending.id, pending.connection.id);
            replies.push(LaterReply::to(&pending, Nip46Response::err(pending.request_id.clone(), approvals::timed_out_message(timeout))));
        }
        let waiting = self.approvals.list(None);
        let mut ended = std::collections::HashSet::new();
        for pending in &waiting {
            match self.db.active_nip46_connection_at(&pending.client_pubkey.to_hex(), now) {
                Ok(Some(c)) if c.id == pending.connection.id => {}
                Ok(_) => {
                    ended.insert(pending.id);
                }
                Err(e) => warn!("Failed to check the connection of approval {}: {}", pending.id, e),
            }
        }
        for pending in self.approvals.take_where(|p| ended.contains(&p.id)) {
            replies.push(self.cancel(pending, now));
        }
        replies
    }

    /// Cancels `pending`, whose connection has ended: recorded as `approval_cancelled`.
    fn cancel(&self, pending: Pending, now: chrono::DateTime<Utc>) -> LaterReply {
        let detail = format!("{};reason=connection_ended", pending.audit_detail());
        if let Err(e) = self.db.record_approval(action::APPROVAL_CANCELLED, &pending.connection, None, detail, now) {
            warn!("Failed to record that approval {} was cancelled: {}", pending.id, e);
        }
        info!("Approval {} cancelled: NIP-46 connection {} has ended", pending.id, pending.connection.id);
        LaterReply::to(&pending, Nip46Response::err(pending.request_id.clone(), approvals::CONNECTION_ENDED))
    }

    /// Returns a request taken for a decision that could not be recorded, so it still waits.
    fn put_back(&self, pending: Pending) {
        if self.approvals.hold(pending).is_err() {
            warn!("An approval could not be put back in the queue, which is full");
        }
    }
}

impl Nip46Handler {
    /// `nip44_*` and `nip04_*` (#97): `[third_party_pubkey, payload]`, with the bunker's key.
    /// Refused without the method's permission, and for a peer that is a NIP-46 app's key: one
    /// that has connected or sent the bunker any request (`Database::is_nip46_client`). A
    /// decrypt is recorded first, and nothing is decrypted if that fails.
    async fn handle_crypto(&self, request: Nip46Request, connection: &Nip46Connection) -> Nip46Response {
        let method = request.method.as_str();
        if !connection.perms.iter().any(|p| p == method) {
            return Nip46Response::err(request.id, format!("Forbidden: this connection may not call {}", method));
        }
        let (Some(peer), Some(payload)) = (request.params.first(), request.params.get(1)) else {
            return Nip46Response::err(request.id, "Missing parameters: expected a pubkey and a payload");
        };
        // NIP-46 sends the third party's key as hex.
        let Ok(peer) = PublicKey::from_hex(peer.trim()) else {
            return Nip46Response::err(request.id, "Invalid pubkey");
        };
        let peer_hex = peer.to_hex();

        let Some(signer) = self.keyring.signer(connection.identity_pubkey.as_deref()).await else {
            return Nip46Response::err(request.id, IDENTITY_UNAVAILABLE);
        };
        // The identity's own key is allowed: NIP-51 private lists encrypt to oneself.
        if peer_hex != signer.public_key_hex() {
            match self.db.is_nip46_client(&peer_hex) {
                Ok(false) => {}
                Ok(true) => return Nip46Response::err(request.id, format!("Forbidden: {} is not allowed with a NIP-46 app key", method)),
                Err(e) => {
                    warn!("Failed to check the peer of a {} from {}: {}", method, connection.client_pubkey, e);
                    return Nip46Response::err(request.id, "Internal error");
                }
            }
        }

        let now = Utc::now();
        let decrypt_action = match method {
            "nip44_decrypt" => Some(action::NIP44_DECRYPT),
            "nip04_decrypt" => Some(action::NIP04_DECRYPT),
            _ => None,
        };
        if let Some(decrypt_action) = decrypt_action {
            if let Err(e) = self.db.record_decrypt(connection, decrypt_action, &peer_hex, now) {
                warn!("Failed to record a {} from {}, so nothing was decrypted: {}", method, connection.client_pubkey, e);
                return Nip46Response::err(request.id, "Internal error");
            }
        }

        let result = match method {
            "nip44_encrypt" => signer.encrypt(&peer, payload),
            "nip44_decrypt" => signer.decrypt(&peer, payload),
            "nip04_encrypt" => signer.nip04_encrypt(&peer, payload),
            _ => signer.nip04_decrypt(&peer, payload),
        };
        drop(signer);
        if let Err(e) = self.db.touch_nip46_connection(&connection.id, now) {
            warn!("Failed to record NIP-46 connection use: {}", e);
        }
        match result {
            // NIP-46 traffic is encrypted with the identity's key, so a decrypt never returns it,
            // whichever peer is named: this covers a request the bunker never received, such as
            // one sent while it was stopped.
            Ok(output) if decrypt_action.is_some() && is_nip46_message(&output) => {
                Nip46Response::err(request.id, format!("Forbidden: {} does not return NIP-46 messages", method))
            }
            Ok(output) => Nip46Response::ok(request.id, output),
            // `Signer` words these without the payload.
            Err(e) => Nip46Response::err(request.id, e.to_string()),
        }
    }
}

/// Whether `plaintext` is a NIP-46 request or response: a JSON object with an `id` and a `method`,
/// `result` or `error`.
fn is_nip46_message(plaintext: &str) -> bool {
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str::<serde_json::Value>(plaintext.trim()) else {
        return false;
    };
    object.contains_key("id") && ["method", "result", "error"].iter().any(|key| object.contains_key(*key))
}

/// NIP-46 client metadata: a JSON object with optional `name`, `url` and `image`. Self-reported
/// and unauthenticated, so kept only as display hints, trimmed and bounded; anything malformed is
/// dropped rather than refusing the connection.
fn parse_metadata(raw: &str) -> ClientMetadata {
    let value: serde_json::Value = serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
    let field = |key: &str, max: usize| {
        value.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(max).collect::<String>())
    };
    let link = |key: &str| field(key, 512).filter(|v| v.starts_with("https://") || v.starts_with("http://"));
    ClientMetadata { name: field("name", 100), url: link("url"), image: link("image") }
}
