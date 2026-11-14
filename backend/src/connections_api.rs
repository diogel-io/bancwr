//! The NIP-46 connection routes (#53), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `POST /api/bunker/connections/tokens` | administrator |
//! | `GET /api/bunker/connections/tokens` | administrator |
//! | `DELETE /api/bunker/connections/tokens/:id` | administrator |
//! | `GET /api/bunker/connections` | administrator and signer: administrators see all, signers their own |
//! | `DELETE /api/bunker/connections/:id` | administrator and signer: administrators any, signers their own (#31) |
//!
//! Viewers hold no connections (#77): a token is only issued for an administrator or a signer.
//!
//! A token's secret is returned once, in the creation response, and only its hash is kept.
use crate::db::{Nip46Connection, Nip46Token};
use crate::nip46::{hash_secret, sign_permission, ENCRYPTION_METHODS};
use crate::proxy_auth::Caller;
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
    pub used_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl From<Nip46Token> for TokenResponse {
    fn from(t: Nip46Token) -> Self {
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
}

impl From<Nip46Connection> for ConnectionResponse {
    fn from(c: Nip46Connection) -> Self {
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
    let (caller_pubkey, _) = caller(who);
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
    let mut kinds: Vec<u16> = Vec::new();
    for kind in &request.kinds {
        let Ok(kind) = u16::try_from(*kind) else {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_kinds", "Kinds are numbers from 0 to 65535."));
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds.sort_unstable();
    for method in &request.methods {
        if !ENCRYPTION_METHODS.contains(&method.as_str()) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_methods",
                "Methods are nip44_encrypt, nip44_decrypt, nip04_encrypt and nip04_decrypt.",
            ));
        }
    }
    let methods = methods(&request.methods);
    if kinds.is_empty() && methods.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_kinds", "Choose at least one kind or method."));
    }
    let hours = request.expires_in_hours.unwrap_or(DEFAULT_TOKEN_HOURS);
    if hours == 0 || hours > MAX_TOKEN_HOURS {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_expiry", "A token can be valid for 1 hour to 7 days."));
    }
    if let Some(lifetime) = request.connection_lifetime_hours {
        if lifetime == 0 || lifetime > MAX_CONNECTION_LIFETIME_HOURS {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_lifetime", "A connection can last 1 hour to 365 days, or until revoked."));
        }
    }

    let for_pubkey = match request.for_pubkey.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(key) => canonical_pubkey(key).map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "That is not a valid npub or hex key."))?,
        None if !caller_pubkey.is_empty() => caller_pubkey.clone(),
        None => return Err(api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "Say which member the connection is for.")),
    };
    match state.db.find_member_by_pubkey(&for_pubkey).map_err(database_error)? {
        // Only a member who can sign: an administrator or a signer, never a viewer (#77).
        Some(member) if member.role().is_some_and(|role| role.can_sign()) => {}
        Some(_) => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "member_cannot_sign",
                "Connections can only be issued for an admin or a signer, not a viewer.",
            ))
        }
        None => return Err(api_error(StatusCode::BAD_REQUEST, "not_registered", "Connections can only be issued for members of the vault.")),
    }

    let secret = new_secret();
    let now = Utc::now();
    let perms: Vec<String> = kinds.iter().map(|k| sign_permission(*k)).chain(methods.iter().cloned()).collect();
    let token = state
        .db
        .create_nip46_token_with_lifetime(
            &hash_secret(&secret),
            &for_pubkey,
            &caller_pubkey,
            label,
            &perms,
            now,
            now + Duration::hours(hours.into()),
            request.connection_lifetime_hours,
        )
        .map_err(database_error)?;

    let bunker = state.signer.read().await.public_key_hex();
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
    }))
}

/// List connection tokens (never their secrets)
/// GET /api/bunker/connections/tokens
pub async fn list_tokens(State(state): State<AppState>) -> Result<Json<Vec<TokenResponse>>, ApiError> {
    let tokens = state.db.list_nip46_tokens().map_err(database_error)?;
    Ok(Json(tokens.into_iter().map(Into::into).collect()))
}

/// Revoke an unused token
/// DELETE /api/bunker/connections/tokens/:id
pub async fn revoke_token(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (by, _) = caller(who);
    if state.db.revoke_nip46_token_as(&id, crate::audit::actor(&by), Utc::now()).map_err(database_error)? {
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
    Ok(Json(connections.into_iter().map(Into::into).collect()))
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
        Ok(Json(json!({ "success": true })))
    } else {
        Err(api_error(StatusCode::NOT_FOUND, "not_found", "No active connection with that id."))
    }
}

