//! The NIP-46 connection routes (#53), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `POST /api/bunker/connections/tokens` | administrator and signer: administrators for any member who can sign, signers only for themselves and without approval kinds (#142) |
//! | `GET /api/bunker/connections/tokens` | administrator and signer: administrators see all, signers their own (#142) |
//! | `DELETE /api/bunker/connections/tokens/:id` | administrator and signer: administrators any unused token, signers their own (#142) |
//! | `GET /api/bunker/connections` | administrator and signer: administrators see all, signers their own |
//! | `DELETE /api/bunker/connections/:id` | administrator and signer: administrators any, signers their own (#31) |
//! | `POST /api/bunker/connections/nostrconnect/preview` | administrator and signer: reads an app's `nostrconnect://` code and changes nothing (#148) |
//! | `POST /api/bunker/connections/nostrconnect` | administrator and signer: connects the app in a `nostrconnect://` code, with the same rules as issuing a token (#148) |
//!
//! A token is only issued for a member with a recognised role (#77, #143): an administrator or a
//! signer. A stored role that is not recognised grants nothing.
//!
//! A token's secret is returned once, in the creation response, and only its hash is kept.
//!
//! An app that offers only a `nostrconnect://` code (#148) is connected from the console instead:
//! the member pastes the code, sees what the app says about itself and asks for, and chooses the
//! identity and what the connection may do, under exactly the rules a token follows. The bunker
//! then sends the `connect` response, with the app's secret, on the app's relays. The secret is
//! never stored, logged or returned.
use crate::db::{ClientMetadata, Nip46Connection, Nip46Token, NostrConnectGrant, NostrConnectRefusal, TokenOptions};
use crate::nostrconnect::{self, NostrConnectUri};
use crate::nip46::{hash_secret, sign_permission, ENCRYPTION_METHODS};
use crate::proxy_auth::Caller;
use crate::rate_limit::RateLimit;
use crate::registry::{canonical_pubkey, Role};
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::{Duration, Utc};
use nostr::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{error, info};

pub const DEFAULT_TOKEN_HOURS: u32 = 24;
pub const MAX_TOKEN_HOURS: u32 = 24 * 7;
/// The longest a connection may be issued to last (#98): 365 days. Longer is "until revoked".
pub const MAX_CONNECTION_LIFETIME_HOURS: u32 = 24 * 365;
pub const MAX_LABEL_CHARS: usize = 100;

pub(crate) type ApiError = (StatusCode, Json<Value>);

pub(crate) fn api_error(status: StatusCode, error: &str, message: &str) -> ApiError {
    (status, Json(json!({ "error": error, "message": message })))
}

pub(crate) fn database_error(e: anyhow::Error) -> ApiError {
    error!("Database error in the connection routes: {}", e);
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error")
}

/// The caller's key and whether they administer. Without the guard (an unauthenticated test
/// router; a running bunker always has it, #11) the caller is treated as an administrator, as the
/// other administration routes are then open too.
pub(crate) fn caller(caller: Option<Extension<Caller>>) -> (String, bool) {
    match caller.map(|Extension(c)| c) {
        Some(Caller::Member { pubkey, role }) => (pubkey, role == Role::Administrator),
        Some(Caller::Service) => (String::new(), false),
        None => (String::new(), true),
    }
}

/// The kinds of `sign_event:<kind>` permissions, for display.
fn kinds(perms: &[String]) -> Vec<u16> {
    perms.iter().filter_map(|p| p.strip_prefix("sign_event:")?.parse().ok()).collect()
}

/// The encryption methods among the permissions (#97), in `ENCRYPTION_METHODS` order.
fn methods(perms: &[String]) -> Vec<String> {
    ENCRYPTION_METHODS.iter().filter(|m| perms.iter().any(|p| p == *m)).map(|m| m.to_string()).collect()
}

#[derive(Deserialize)]
pub struct IssueTokenRequest {
    /// The member the connection is for (npub or hex); the caller when omitted.
    pub for_pubkey: Option<String>,
    pub label: String,
    /// Kinds the connection may sign.
    #[serde(default)]
    pub kinds: Vec<u32>,
    /// Encryption methods it may call (#97): any of `ENCRYPTION_METHODS`. None when omitted.
    #[serde(default)]
    pub methods: Vec<String>,
    pub expires_in_hours: Option<u32>,
    /// How long the connection lasts once the app connects (#98), 1 hour to 365 days. None: until
    /// revoked.
    #[serde(default)]
    pub connection_lifetime_hours: Option<u32>,
    /// The connection's rate limit (#101): requests a minute, 1 to 600, and burst, 1 to 100. Each
    /// defaults on its own, to 60 and 20 (`RateLimit::DEFAULT`).
    #[serde(default)]
    pub rate_per_minute: Option<u32>,
    #[serde(default)]
    pub rate_burst: Option<u32>,
    /// Kinds held for an administrator's approval before signing (#102): each one of `kinds`.
    #[serde(default)]
    pub approval_kinds: Vec<u32>,
    /// The identity the connection signs as (#103): npub or hex. The primary when omitted.
    #[serde(default)]
    pub identity: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IssueTokenResponse {
    pub id: String,
    /// `bunker://<pubkey>?relay=…&secret=…`: shown once, never again.
    pub uri: String,
    pub for_pubkey: String,
    pub kinds: Vec<u16>,
    /// Encryption methods it may call (#97).
    pub methods: Vec<String>,
    pub expires_at: String,
    /// How long the connection lasts once the app connects (#98). None: until revoked.
    pub connection_lifetime_hours: Option<u32>,
    /// The connection's rate limit in force (#101).
    pub rate_per_minute: u32,
    pub rate_burst: u32,
    /// Kinds held for approval (#102).
    pub approval_kinds: Vec<u16>,
    /// The identity the connection signs as (#103), hex.
    pub identity_pubkey: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TokenResponse {
    pub id: String,
    pub for_pubkey: String,
    pub issued_by: String,
    pub label: String,
    pub kinds: Vec<u16>,
    pub methods: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
    /// How long the connection lasts once the app connects (#98). None: until revoked.
    pub connection_lifetime_hours: Option<u32>,
    /// The connection's rate limit in force (#101).
    pub rate_per_minute: u32,
    pub rate_burst: u32,
    /// Kinds held for approval (#102).
    pub approval_kinds: Vec<u16>,
    /// The identity the connection signs as (#103), hex. Filled in by the route: None only before.
    pub identity_pubkey: Option<String>,
    pub used_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl From<Nip46Token> for TokenResponse {
    fn from(t: Nip46Token) -> Self {
        let limit = RateLimit::effective(t.rate_limit);
        Self {
            id: t.id,
            for_pubkey: t.for_pubkey,
            issued_by: t.issued_by,
            label: t.label,
            kinds: kinds(&t.perms),
            methods: methods(&t.perms),
            created_at: t.created_at.to_rfc3339(),
            expires_at: t.expires_at.to_rfc3339(),
            connection_lifetime_hours: t.connection_lifetime_hours,
            rate_per_minute: limit.per_minute,
            rate_burst: limit.burst,
            approval_kinds: kinds(&t.approval_perms),
            identity_pubkey: t.identity_pubkey,
            used_at: t.used_at.map(|v| v.to_rfc3339()),
            revoked_at: t.revoked_at.map(|v| v.to_rfc3339()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConnectionResponse {
    pub id: String,
    pub client_pubkey: String,
    pub for_pubkey: String,
    /// NIP-46 client metadata: what the app says about itself. Never verified.
    pub client_name: Option<String>,
    pub client_url: Option<String>,
    pub client_image: Option<String>,
    pub metadata_verified: bool,
    pub kinds: Vec<u16>,
    pub methods: Vec<String>,
    pub connected_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub revoked_reason: Option<String>,
    /// Who revoked it (hex), when someone did (#31).
    pub revoked_by: Option<String>,
    /// When it stops working (#98). None: until revoked.
    pub expires_at: Option<String>,
    /// Its rate limit in force (#101).
    pub rate_per_minute: u32,
    pub rate_burst: u32,
    /// Kinds held for approval (#102).
    pub approval_kinds: Vec<u16>,
    /// The identity it signs as (#103), hex. Filled in by the route: None only before.
    pub identity_pubkey: Option<String>,
    /// The relays the app chose, when it connected from its own `nostrconnect://` code (#148):
    /// the bunker listens there too while the connection lasts. Empty for a token's connection.
    #[serde(default)]
    pub app_relays: Vec<String>,
}

impl From<Nip46Connection> for ConnectionResponse {
    fn from(c: Nip46Connection) -> Self {
        let limit = c.effective_rate_limit();
        Self {
            id: c.id,
            client_pubkey: c.client_pubkey,
            for_pubkey: c.for_pubkey,
            client_name: c.client_name,
            client_url: c.client_url,
            client_image: c.client_image,
            metadata_verified: false,
            kinds: kinds(&c.perms),
            methods: methods(&c.perms),
            connected_at: c.connected_at.to_rfc3339(),
            last_used_at: c.last_used_at.map(|v| v.to_rfc3339()),
            revoked_at: c.revoked_at.map(|v| v.to_rfc3339()),
            revoked_reason: c.revoked_reason,
            revoked_by: c.revoked_by,
            expires_at: c.expires_at.map(|v| v.to_rfc3339()),
            rate_per_minute: limit.per_minute,
            rate_burst: limit.burst,
            approval_kinds: kinds(&c.approval_perms),
            identity_pubkey: c.identity_pubkey,
            app_relays: c.relays,
        }
    }
}

/// Percent-encodes a query parameter value (RFC 3986 unreserved characters pass through).
fn query_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// A random 32-byte secret, hex. The secp256k1 secret-key generator is the crate's CSPRNG source.
fn new_secret() -> String {
    SecretKey::generate().to_secret_hex()
}

/// Issue a connection token
/// POST /api/bunker/connections/tokens
pub async fn issue_token(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Json(request): Json<IssueTokenRequest>,
) -> Result<Json<IssueTokenResponse>, ApiError> {
    let (caller_pubkey, administrator) = caller(who);
    // The relays in force (#78): NIP46_RELAYS, or the list saved in the console. A token always
    // carries the current list.
    let enabled = state.config.read().await.nip46_enabled;
    let relays = crate::bunker_relays::effective_relays(&state).await.map_err(database_error)?.urls;
    if !enabled || relays.is_empty() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "nip46_disabled",
            "NIP-46 is turned off (NIP46_ENABLED), or no relays are configured: set them under Config, Bunker relays, or with NIP46_RELAYS.",
        ));
    }

    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_label", "Give the token a label of up to 100 characters."));
    }
    let hours = request.expires_in_hours.unwrap_or(DEFAULT_TOKEN_HOURS);
    let Grant { for_pubkey, identity, kinds, methods, connection_lifetime_hours, rate_limit, approval_kinds } = check_grant(
        &state,
        &caller_pubkey,
        administrator,
        GrantRequest {
            for_pubkey: request.for_pubkey.as_deref(),
            kinds: &request.kinds,
            methods: &request.methods,
            expires_in_hours: Some(hours),
            connection_lifetime_hours: request.connection_lifetime_hours,
            rate_per_minute: request.rate_per_minute,
            rate_burst: request.rate_burst,
            approval_kinds: &request.approval_kinds,
            identity: request.identity.as_deref(),
        },
    )
    .await?;

    let secret = new_secret();
    let now = Utc::now();
    let perms: Vec<String> = kinds.iter().map(|k| sign_permission(*k)).chain(methods.iter().cloned()).collect();
    let token = state
        .db
        .create_nip46_token_with(
            &hash_secret(&secret),
            &for_pubkey,
            &caller_pubkey,
            label,
            &perms,
            now,
            now + Duration::hours(hours.into()),
            TokenOptions {
                connection_lifetime_hours,
                rate_limit,
                approval_perms: approval_kinds.iter().map(|k| sign_permission(*k)).collect(),
                identity_pubkey: identity.clone(),
            },
        )
        .map_err(database_error)?;

    // The remote signer an app talks to is the identity's own key.
    let bunker = match state.keyring.signer(identity.as_deref()).await {
        Some(signer) => signer.public_key_hex(),
        None => return Err(api_error(StatusCode::BAD_REQUEST, "invalid_identity", "That is not one of this bunker's identities.")),
    };
    let relay_params: String = relays.iter().map(|r| format!("relay={}&", query_encode(r))).collect();
    info!("Issued NIP-46 connection token {} for {} with {}", token.id, for_pubkey, perms.join(","));
    Ok(Json(IssueTokenResponse {
        id: token.id,
        uri: format!("bunker://{}?{}secret={}", bunker, relay_params, secret),
        for_pubkey,
        kinds,
        methods,
        expires_at: token.expires_at.to_rfc3339(),
        connection_lifetime_hours: token.connection_lifetime_hours,
        rate_per_minute: RateLimit::effective(token.rate_limit).per_minute,
        rate_burst: RateLimit::effective(token.rate_limit).burst,
        approval_kinds,
        identity_pubkey: bunker.clone(),
    }))
}

/// List connection tokens (never their secrets): an administrator sees every one, a signer only
/// those issued for their key (#142).
/// GET /api/bunker/connections/tokens
pub async fn list_tokens(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
) -> Result<Json<Vec<TokenResponse>>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let scope = if administrator { None } else { Some(pubkey.as_str()) };
    let tokens = state.db.list_nip46_tokens_for(scope).map_err(database_error)?;
    let primary = state.signer.read().await.public_key_hex();
    Ok(Json(
        tokens
            .into_iter()
            .map(|token| {
                let mut response: TokenResponse = token.into();
                response.identity_pubkey.get_or_insert_with(|| primary.clone());
                response
            })
            .collect(),
    ))
}

/// Revoke an unused token: an administrator any, a signer only one issued for their key (#142).
/// Someone else's token is a 404, as an unknown one is, so its existence is not revealed.
/// DELETE /api/bunker/connections/tokens/:id
pub async fn revoke_token(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (by, administrator) = caller(who);
    let only_for = if administrator { None } else { Some(by.as_str()) };
    if state.db.revoke_nip46_token_scoped(&id, crate::audit::actor(&by), only_for, Utc::now()).map_err(database_error)? {
        info!("Revoked NIP-46 connection token {}", id);
        Ok(Json(json!({ "success": true })))
    } else {
        Err(api_error(StatusCode::NOT_FOUND, "not_found", "No unused token with that id."))
    }
}

/// List connections: an administrator sees every one, anyone else only those for their key.
/// GET /api/bunker/connections
pub async fn list_connections(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
) -> Result<Json<Vec<ConnectionResponse>>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let scope = if administrator { None } else { Some(pubkey.as_str()) };
    // Mark any that have expired (#98), so none is listed as active after its time.
    crate::nip46::expire_connections(&state.db);
    let connections = state.db.list_nip46_connections(scope).map_err(database_error)?;
    let primary = state.signer.read().await.public_key_hex();
    Ok(Json(
        connections
            .into_iter()
            .map(|connection| {
                let mut response: ConnectionResponse = connection.into();
                response.identity_pubkey.get_or_insert_with(|| primary.clone());
                response
            })
            .collect(),
    ))
}

/// Revoke a connection: an administrator any, anyone else only one made for their key (#31).
/// Someone else's connection is a 404, as an unknown one is, so its existence is not revealed.
/// DELETE /api/bunker/connections/:id
pub async fn revoke_connection(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let revoked_by = (!pubkey.is_empty()).then_some(pubkey.as_str());
    let only_for = if administrator { None } else { Some(pubkey.as_str()) };
    if state.db.revoke_nip46_connection_as(&id, "revoked", revoked_by, only_for, Utc::now()).map_err(database_error)? {
        info!("Revoked NIP-46 connection {} (by {})", id, if pubkey.is_empty() { "-" } else { &pubkey });
        // An app connected from its own code (#148) no longer needs its relays.
        crate::relay::sync_app_relays(&state).await;
        Ok(Json(json!({ "success": true })))
    } else {
        Err(api_error(StatusCode::NOT_FOUND, "not_found", "No active connection with that id."))
    }
}


/// What a connection may do, as a token or the console asks for it: the fields `issue_token` and
/// `connect_nostrconnect` share (#148).
struct GrantRequest<'a> {
    for_pubkey: Option<&'a str>,
    kinds: &'a [u32],
    methods: &'a [String],
    /// A token's own validity, checked when given. None: no token is issued.
    expires_in_hours: Option<u32>,
    connection_lifetime_hours: Option<u32>,
    rate_per_minute: Option<u32>,
    rate_burst: Option<u32>,
    approval_kinds: &'a [u32],
    identity: Option<&'a str>,
}

/// A checked `GrantRequest`.
struct Grant {
    /// The member it is for (hex).
    for_pubkey: String,
    /// The identity it signs as (hex). None: the primary.
    identity: Option<String>,
    kinds: Vec<u16>,
    methods: Vec<String>,
    connection_lifetime_hours: Option<u32>,
    rate_limit: Option<RateLimit>,
    approval_kinds: Vec<u16>,
}

/// Checks what a connection may do, and for whom, as a token is checked (#53, #98, #101, #102,
/// #103, #142): an administrator for any member who can sign, as any identity that member may use;
/// a signer only for themselves, only as identities they may use, and without approval kinds.
async fn check_grant(state: &AppState, caller_pubkey: &str, administrator: bool, request: GrantRequest<'_>) -> Result<Grant, ApiError> {
    let mut kinds: Vec<u16> = Vec::new();
    for kind in request.kinds {
        let Ok(kind) = u16::try_from(*kind) else {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_kinds", "Kinds are numbers from 0 to 65535."));
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds.sort_unstable();
    for method in request.methods {
        if !ENCRYPTION_METHODS.contains(&method.as_str()) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_methods",
                "Methods are nip44_encrypt, nip44_decrypt, nip04_encrypt and nip04_decrypt.",
            ));
        }
    }
    let methods = methods(request.methods);
    if kinds.is_empty() && methods.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_kinds", "Choose at least one kind or method."));
    }
    if let Some(hours) = request.expires_in_hours {
        if hours == 0 || hours > MAX_TOKEN_HOURS {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_expiry", "A token can be valid for 1 hour to 7 days."));
        }
    }
    if let Some(lifetime) = request.connection_lifetime_hours {
        if lifetime == 0 || lifetime > MAX_CONNECTION_LIFETIME_HOURS {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_lifetime", "A connection can last 1 hour to 365 days, or until revoked."));
        }
    }
    let rate_limit = RateLimit::from_parts(request.rate_per_minute, request.rate_burst).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_rate_limit",
            "A connection can make 1 to 600 requests a minute, with a burst of 1 to 100.",
        )
    })?;
    let mut approval_kinds: Vec<u16> = Vec::new();
    for kind in request.approval_kinds {
        match u16::try_from(*kind).ok().filter(|k| kinds.contains(k)) {
            Some(kind) if !approval_kinds.contains(&kind) => approval_kinds.push(kind),
            Some(_) => {}
            None => {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_approval_kinds",
                    "Only kinds the app may sign can need approval.",
                ))
            }
        }
    }
    approval_kinds.sort_unstable();
    // Approval (#102) is an administrator's control: a signer connecting for themselves sets none (#142).
    if !administrator && !approval_kinds.is_empty() {
        return Err(api_error(StatusCode::FORBIDDEN, "approval_admin_only", "Only an administrator can require approval."));
    }

    let for_pubkey = match request.for_pubkey.map(str::trim).filter(|v| !v.is_empty()) {
        Some(key) => canonical_pubkey(key).map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "That is not a valid npub or hex key."))?,
        None if !caller_pubkey.is_empty() => caller_pubkey.to_string(),
        None => return Err(api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "Say which member the connection is for.")),
    };
    // A signer connects only for themselves (#142); an administrator for any member who can sign.
    if !administrator && for_pubkey != caller_pubkey {
        return Err(api_error(StatusCode::FORBIDDEN, "forbidden_member", "You can only issue connections for yourself."));
    }
    // The identity it signs as (#103): the primary unless another is named, and only one this
    // bunker holds and the member may use.
    let identity: Option<String> = match request.identity.map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(value) => {
            let invalid = || api_error(StatusCode::BAD_REQUEST, "invalid_identity", "That is not one of this bunker's identities.");
            let key = PublicKey::parse(value).map_err(|_| invalid())?;
            state.keyring.identity_of(&key).await.ok_or_else(invalid)?
        }
    };
    match state.db.find_member_by_pubkey(&for_pubkey).map_err(database_error)? {
        // Only a member who can sign: one with a recognised role, an administrator or a signer
        // (#77, #143).
        Some(member) if member.role().is_some() => {}
        Some(_) => return Err(api_error(StatusCode::BAD_REQUEST, "member_cannot_sign", "This member has no recognised role and cannot sign.")),
        None => return Err(api_error(StatusCode::BAD_REQUEST, "not_registered", "Connections can only be issued for members of the vault.")),
    }
    if !state.db.may_use_identity(&for_pubkey, identity.as_deref()).map_err(database_error)? {
        return Err(api_error(StatusCode::BAD_REQUEST, "no_identity_access", "That member may not sign as this identity. Grant it on their team page first."));
    }
    Ok(Grant { for_pubkey, identity, kinds, methods, connection_lifetime_hours: request.connection_lifetime_hours, rate_limit, approval_kinds })
}

#[derive(Deserialize)]
pub struct NostrConnectPreviewRequest {
    pub uri: String,
}

/// One of the relays an app chose (#148), and whether it is one of the bunker's own.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct AppRelay {
    pub url: String,
    pub bunker: bool,
}

/// What a `nostrconnect://` code says (#148), for the member to see before they confirm. All of
/// it comes from the app and none of it is verified. Never the secret, and never an image.
#[derive(Serialize, Deserialize, Debug)]
pub struct NostrConnectPreview {
    /// The app's NIP-46 key, hex.
    pub client_pubkey: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub metadata_verified: bool,
    pub relays: Vec<AppRelay>,
    /// What the app asked for, to pre-fill the form. `requested_any_kind`: `sign_event` with no kind.
    pub requested_kinds: Vec<u16>,
    pub requested_any_kind: bool,
    pub requested_methods: Vec<String>,
    /// Whether that key already has an active connection, which would be refused.
    pub already_connected: bool,
}

/// Reads an app's `nostrconnect://` code, or refuses it (#148): a 400 with the code's problem.
/// Its relays may be any of the bunker's own, as well as `wss://` or `ws://` on this machine. It
/// must not name one of the bunker's own identities as the app.
async fn read_code(state: &AppState, uri: &str) -> Result<NostrConnectUri, ApiError> {
    let bunker = crate::bunker_relays::effective_relays(state).await.map_err(database_error)?.urls;
    let code = nostrconnect::parse_with(uri, &bunker).map_err(|refusal| api_error(StatusCode::BAD_REQUEST, "invalid_nostrconnect_uri", refusal.message()))?;
    if state.keyring.identity_of(&code.client_pubkey).await.is_some() {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_nostrconnect_uri", "That code names one of this bunker's own identities as the app."));
    }
    Ok(code)
}

/// Refuses a code while NIP-46 is off: the bunker could not answer the app.
async fn nostrconnect_enabled(state: &AppState) -> Result<crate::relay::RelayHandle, ApiError> {
    let enabled = state.config.read().await.nip46_enabled;
    match state.relay_handle().await {
        Some(handle) if enabled => Ok(handle),
        _ => Err(api_error(StatusCode::CONFLICT, "nip46_disabled", "NIP-46 is turned off (NIP46_ENABLED), so the bunker cannot answer an app.")),
    }
}

/// Read a `nostrconnect://` code without acting on it (#148).
/// POST /api/bunker/connections/nostrconnect/preview
pub async fn preview_nostrconnect(
    State(state): State<AppState>,
    Json(request): Json<NostrConnectPreviewRequest>,
) -> Result<Json<NostrConnectPreview>, ApiError> {
    let code = read_code(&state, &request.uri).await?;
    let bunker = crate::bunker_relays::effective_relays(&state).await.map_err(database_error)?.urls;
    let already_connected = state.db.active_nip46_connection(&code.client_pubkey.to_hex()).map_err(database_error)?.is_some();
    Ok(Json(NostrConnectPreview {
        client_pubkey: code.client_pubkey.to_hex(),
        name: code.name,
        url: code.url,
        metadata_verified: false,
        relays: code.relays.iter().map(|url| AppRelay { url: url.clone(), bunker: bunker.contains(url) }).collect(),
        requested_kinds: code.requested.kinds.clone(),
        requested_any_kind: code.requested.any_kind,
        requested_methods: code.requested.methods.clone(),
        already_connected,
    }))
}

#[derive(Deserialize)]
pub struct NostrConnectRequest {
    /// The app's `nostrconnect://` code.
    pub uri: String,
    /// As for a token (`IssueTokenRequest`).
    pub for_pubkey: Option<String>,
    #[serde(default)]
    pub kinds: Vec<u32>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub connection_lifetime_hours: Option<u32>,
    #[serde(default)]
    pub rate_per_minute: Option<u32>,
    #[serde(default)]
    pub rate_burst: Option<u32>,
    #[serde(default)]
    pub approval_kinds: Vec<u32>,
    #[serde(default)]
    pub identity: Option<String>,
}

/// Connect the app in a `nostrconnect://` code (#148): checked as a token is, granted only what
/// the member confirmed and the app asked for, made in one transaction, then answered with the
/// `connect` response on the app's relays. If none of them takes it, the connection is revoked
/// at once, so none is left that the app never learnt of.
/// POST /api/bunker/connections/nostrconnect
pub async fn connect_nostrconnect(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Json(request): Json<NostrConnectRequest>,
) -> Result<(StatusCode, Json<ConnectionResponse>), ApiError> {
    let (caller_pubkey, administrator) = caller(who);
    let handle = nostrconnect_enabled(&state).await?;
    let code = read_code(&state, &request.uri).await?;
    let Grant { for_pubkey, identity, kinds, methods, connection_lifetime_hours, rate_limit, approval_kinds } = check_grant(
        &state,
        &caller_pubkey,
        administrator,
        GrantRequest {
            for_pubkey: request.for_pubkey.as_deref(),
            kinds: &request.kinds,
            methods: &request.methods,
            expires_in_hours: None,
            connection_lifetime_hours: request.connection_lifetime_hours,
            rate_per_minute: request.rate_per_minute,
            rate_burst: request.rate_burst,
            approval_kinds: &request.approval_kinds,
            identity: request.identity.as_deref(),
        },
    )
    .await?;

    // Only what the member confirmed, and of that only what the app asked for when it asked for
    // anything: the app's own list never adds to the member's.
    let kinds: Vec<u16> = kinds.into_iter().filter(|kind| code.requested.allows_kind(*kind)).collect();
    let methods: Vec<String> = methods.into_iter().filter(|method| code.requested.allows_method(method)).collect();
    if kinds.is_empty() && methods.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "nothing_grantable", "None of the kinds or methods you chose are ones the app asked for."));
    }
    let perms: Vec<String> = kinds.iter().map(|k| sign_permission(*k)).chain(methods.iter().cloned()).collect();
    let approval_perms: Vec<String> = approval_kinds.iter().filter(|k| kinds.contains(k)).map(|k| sign_permission(*k)).collect();

    let Some(signer) = state.keyring.signer(identity.as_deref()).await else {
        return Err(api_error(StatusCode::CONFLICT, "identity_unavailable", "That identity is not available in this bunker yet: it loads when the bunker restarts."));
    };
    drop(signer);

    let client = code.client_pubkey.to_hex();
    let secret_hash = hash_secret(&new_secret());
    let grant = NostrConnectGrant {
        for_pubkey: &for_pubkey,
        made_by: &caller_pubkey,
        client_pubkey: &client,
        perms: &perms,
        options: TokenOptions { connection_lifetime_hours, rate_limit, approval_perms, identity_pubkey: identity.clone() },
        metadata: ClientMetadata { name: code.name.clone(), url: code.url.clone(), image: None },
        relays: &code.relays,
        secret_hash: &secret_hash,
    };
    let connection = match state.db.create_nostrconnect_connection(&grant, Utc::now()).map_err(database_error)? {
        Ok(connection) => connection,
        Err(NostrConnectRefusal::AlreadyConnected) => {
            return Err(api_error(StatusCode::CONFLICT, "already_connected", "That app is already connected. Revoke its connection first, or ask the app for a new code."))
        }
        Err(NostrConnectRefusal::MemberRemoved) => return Err(api_error(StatusCode::BAD_REQUEST, "not_registered", "Connections can only be issued for members of the vault.")),
        Err(NostrConnectRefusal::MemberCannotSign) => {
            return Err(api_error(StatusCode::BAD_REQUEST, "member_cannot_sign", "This member has no recognised role and cannot sign."))
        }
        Err(NostrConnectRefusal::NoIdentityAccess) => {
            return Err(api_error(StatusCode::BAD_REQUEST, "no_identity_access", "That member may not sign as this identity. Grant it on their team page first."))
        }
    };

    // Listen on the app's relays before answering: it asks for switch_relays straight away.
    crate::relay::sync_app_relays(&state).await;
    let sent = match state.keyring.signer(identity.as_deref()).await {
        Some(signer) => handle.send_connect(&signer, code.client_pubkey, &code.secret, &code.relays).await,
        None => Err(anyhow::anyhow!("the identity is not unlocked")),
    };
    if let Err(e) = sent {
        tracing::warn!("The connect response to {} reached none of its relays, so its new connection {} is revoked: {}", client, connection.id, e);
        if let Err(e) = state.db.revoke_nip46_connection(&connection.id, crate::db::CONNECT_UNDELIVERED_REASON, Utc::now()) {
            error!("Failed to revoke the undelivered NIP-46 connection {}: {}", connection.id, e);
        }
        crate::relay::sync_app_relays(&state).await;
        return Err(api_error(
            StatusCode::BAD_GATEWAY,
            "connect_undelivered",
            "None of the app's relays took the bunker's answer, so nothing was connected. Check the app is still waiting, or ask it for a new code.",
        ));
    }

    info!("Connected NIP-46 client {} for {} from its nostrconnect:// code, with {}", client, for_pubkey, perms.join(","));
    let primary = state.signer.read().await.public_key_hex();
    let mut response: ConnectionResponse = connection.into();
    response.identity_pubkey.get_or_insert(primary);
    Ok((StatusCode::CREATED, Json(response)))
}
