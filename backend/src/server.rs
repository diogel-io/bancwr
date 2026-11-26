use crate::health::{Check, Overall};
use crate::registry::{canonical_pubkey, Role};
use crate::state::AppState;
use crate::db::{RemoveOutcome, RoleChangeOutcome};
use crate::proxy_auth::{Access, Guard, Nonces};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use nostr::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use uuid::Uuid;

const HEALTH_PATH: &str = "/health";
const STATUS_PATH: &str = "/api/bunker/status";

// Removed local AppState struct as it is now in state.rs
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct HealthResponse {
    /// `ok`, or `locked` when the bunker could not unlock its key (#94).
    pub status: String,
    /// Why the bunker is locked: fixed wording, never a path or key material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BunkerStatus {
    /// Healthy, degraded or unhealthy, from the checks (#27).
    pub status: Overall,
    /// The primary identity's npub.
    pub pubkey: String,
    /// Every identity's key (#103), hex, the primary first. Sign-in refuses each of them.
    #[serde(default)]
    pub identities: Vec<String>,
    /// The running Bancwr version, from BANCWR_VERSION (#35). Deliberately not on /health, which
    /// stays cheap and dependency-free for container probes.
    pub version: String,
    /// Each check's result, naming what needs attention (#27).
    pub checks: Vec<Check>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LogEntry {
    pub id: String,
    pub event_id: String,
    /// The key that asked for the signature: the NIP-46 client's (hex), not the member's.
    pub pubkey: String,
    pub event_kind: u32,
    pub timestamp: String, // ISO 8601 format
    /// The member the signature was for (hex), when known (diogel-io/workspace#38).
    pub member_pubkey: Option<String>,
    /// That member's name, while they are still in the vault.
    pub member_name: Option<String>,
    pub connection_id: Option<String>,
    /// The identity that signed (#103), hex.
    pub identity_pubkey: Option<String>,
    /// `console` when an administrator's edit in the console asked for it (#150), not an app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Metrics {
    pub http_requests: u64,
    pub nip46_connections: usize,
    pub total_signatures: u64,
    /// NIP-46 requests refused for a connection's rate limit since the bunker started (#101).
    pub nip46_rate_limited: u64,
    /// NIP-46 requests waiting for an administrator's approval now (#102).
    pub approvals_pending: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigResponse {
    pub pubkey: String,
    /// How the signing key is stored: always `encrypted` since #94. Never the key, or a path.
    pub key_storage: String,
}

/// `ConfigResponse::key_storage` since #94.
pub const KEY_STORAGE_ENCRYPTED: &str = "encrypted";

#[derive(Serialize, Deserialize, Debug)]
pub struct TeamMemberResponse {
    pub id: String,
    pub name: String,
    /// Canonical lowercase hex (#24).
    pub pubkey: String,
    /// For display. `None` only for a row stored before #24 whose key is not valid.
    pub npub: Option<String>,
    /// `administrator` or `signer` (#77, #143); a row from before #24 may hold another value.
    pub role: String,
}

impl From<crate::db::TeamMember> for TeamMemberResponse {
    fn from(m: crate::db::TeamMember) -> Self {
        TeamMemberResponse {
            id: m.id.to_string(),
            npub: crate::registry::npub(&m.pubkey),
            name: m.name,
            pubkey: m.pubkey,
            role: m.role,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddTeamMemberRequest {
    pub name: String,
    pub pubkey: String,
    pub role: String,
    /// The identities the member may sign as (#103), hex or npub, when the bunker has several.
    /// Omitted: the primary. Ignored with only the primary, which everyone may use.
    #[serde(default)]
    pub identities: Option<Vec<String>>,
}

/// The body of `PATCH /api/bunker/team/:id` (#99).
#[derive(Serialize, Deserialize, Debug)]
pub struct ChangeRoleRequest {
    pub role: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TeamOperationResponse {
    pub success: bool,
    pub message: String,
}

/// Health check endpoint
/// GET /health
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        reason: None,
    })
}

/// Status endpoint
/// GET /api/bunker/status
pub async fn get_status(
    State(state): State<AppState>,
) -> Json<BunkerStatus> {
    state.increment_http_request_count();
    // A report, not a probe: 200 in every state. Probes use /health.
    let (status, checks) = crate::health::report(&state).await;
    Json(BunkerStatus {
        status,
        pubkey: state.signer.read().await.public_key_bech32(),
        identities: state.keyring.public_keys().await.iter().map(|key| key.to_hex()).collect(),
        version: state.config.read().await.version.clone(),
        checks,
    })
}

/// Get metrics endpoint
/// GET /api/bunker/metrics
pub async fn get_metrics(
    State(state): State<AppState>,
) -> Json<Metrics> {
    Json(Metrics {
        http_requests: state.http_request_count(),
        nip46_connections: state.nip46_handler.connection_count().await,
        total_signatures: state.db.signature_count().unwrap_or(0),
        nip46_rate_limited: state.nip46_handler.rate_limited_count(),
        approvals_pending: state.nip46_handler.approvals_pending(),
    })
}

impl From<crate::db::SigningLog> for LogEntry {
    fn from(log: crate::db::SigningLog) -> Self {
        LogEntry {
            id: log.id.to_string(),
            event_id: log.event_id,
            pubkey: log.pubkey,
            event_kind: log.event_kind,
            timestamp: log.timestamp.to_rfc3339(),
            member_pubkey: log.member_pubkey,
            member_name: log.member_name,
            connection_id: log.connection_id,
            identity_pubkey: log.identity_pubkey,
            origin: log.origin,
        }
    }
}

/// How many entries `GET /api/bunker/logs/mine` returns: enough for the signer dashboard (#77).
pub const MY_LOGS_LIMIT: usize = 20;

/// The caller's own recent signatures (#77), for the signer dashboard: those attributed to the
/// caller as the member (diogel-io/workspace#38), newest first. Administrators and signers; an
/// administrator sees only their own here too, and everyone's on /logs. Without the guard (an
/// unauthenticated test router) there is no caller, so nothing is theirs.
/// GET /api/bunker/logs/mine
pub async fn get_my_logs(
    State(state): State<AppState>,
    who: Option<axum::Extension<crate::proxy_auth::Caller>>,
) -> Result<Json<Vec<LogEntry>>, (StatusCode, String)> {
    let Some(axum::Extension(crate::proxy_auth::Caller::Member { pubkey, .. })) = who else {
        return Ok(Json(Vec::new()));
    };
    match state.db.get_recent_logs_for_member(&pubkey, MY_LOGS_LIMIT) {
        Ok(logs) => Ok(Json(logs.into_iter().map(Into::into).collect())),
        Err(e) => {
            error!("Failed to fetch the caller's logs: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))
        }
    }
}

/// Get current configuration
/// GET /api/bunker/config
pub async fn get_config(
    State(state): State<AppState>,
) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        pubkey: state.signer.read().await.public_key_bech32(),
        key_storage: KEY_STORAGE_ENCRYPTED.to_string(),
    })
}

/// Get team members
/// GET /api/bunker/team
pub async fn get_team(
    State(state): State<AppState>,
) -> Result<Json<Vec<TeamMemberResponse>>, (StatusCode, String)> {
    match state.db.get_team_members() {
        Ok(members) => {
            let response: Vec<TeamMemberResponse> = members.into_iter().map(Into::into).collect();
            Ok(Json(response))
        }
        Err(e) => {
            error!("Failed to fetch team: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))
        }
    }
}

/// Add team member
/// POST /api/bunker/team
pub async fn add_team_member(
    State(state): State<AppState>,
    who: Option<axum::Extension<crate::proxy_auth::Caller>>,
    Json(request): Json<AddTeamMemberRequest>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    // The two roles of #143: administrator and signer. Older names (`admin`, `user`, and #77's
    // third role) are refused with 400.
    let role: Role = request
        .role
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid role".to_string()))?;

    // Any valid key, as an npub or hex, stored in one canonical form.
    let pubkey = canonical_pubkey(&request.pubkey)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid pubkey".to_string()))?;

    // Never the bunker's own key: anyone holding a NIP-46 connection can obtain its signature
    // (since #53, only within their granted kinds), so it must never sign in. See the sign-in
    // decision record, rule 10.
    // The same for every identity it holds (#103), including one added since it started.
    let stored_identity = state.db.find_identity(&pubkey).map_err(|e| {
        error!("Failed to look up identities: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string())
    })?;
    if state.keyring.is_identity(&pubkey).await || stored_identity.is_some() {
        return Err((StatusCode::BAD_REQUEST, "The bunker's own key cannot be registered".to_string()));
    }

    // Which identities they may sign as (#103), checked before anything is stored.
    let several = state.db.has_several_identities().map_err(|e| {
        error!("Failed to read identities: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string())
    })?;
    let mut grants: Vec<Option<String>> = Vec::new();
    if several {
        match &request.identities {
            None => grants.push(None),
            Some(keys) => {
                for key in keys {
                    let parsed = PublicKey::parse(key.trim()).map_err(|_| (StatusCode::BAD_REQUEST, "Invalid identity".to_string()))?;
                    let identity = state.keyring.identity_of(&parsed).await.ok_or((StatusCode::BAD_REQUEST, "Invalid identity".to_string()))?;
                    if !grants.contains(&identity) {
                        grants.push(identity);
                    }
                }
            }
        }
    }

    match state.db.find_member_by_pubkey(&pubkey) {
        Ok(Some(_)) => return Err((StatusCode::CONFLICT, "This key is already registered".to_string())),
        Ok(None) => {}
        Err(e) => {
            error!("Failed to look up team member: {}", e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()));
        }
    }

    let (by, _) = crate::connections_api::caller(who);
    match state.db.add_team_member_as(&request.name, &pubkey, role, crate::audit::actor(&by)) {
        Ok(_) => {
            info!("Added team member: {}", request.name);
            for identity in &grants {
                if let Err(e) = state.db.grant_identity(identity.as_deref(), &pubkey, crate::audit::actor(&by), chrono::Utc::now()) {
                    error!("Failed to grant a new member an identity: {}", e);
                }
            }
            Ok(Json(TeamOperationResponse {
                success: true,
                message: "Team member added".to_string(),
            }))
        }
        Err(e) => {
            error!("Failed to add team member: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

/// Look up one member by key
/// GET /api/bunker/team/by-pubkey/:pubkey
///
/// Takes an npub or hex. 200 with the member, 404 `{"error":"not_registered"}` for a key not in
/// the vault, 400 for a value that is not a key. The lookup #25 and #11 authorise against (#24).
pub async fn get_team_member_by_pubkey(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<TeamMemberResponse>, (StatusCode, Json<serde_json::Value>)> {
    lookup_member(&state.db, &key)
}

fn lookup_member(
    db: &crate::db::Database,
    key: &str,
) -> Result<Json<TeamMemberResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pubkey = canonical_pubkey(key).map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "invalid_pubkey" })))
    })?;
    match db.find_member_by_pubkey(&pubkey) {
        Ok(Some(member)) => Ok(Json(member.into())),
        Ok(None) => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "not_registered" })))),
        Err(e) => {
            error!("Failed to look up team member: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "database_error" }))))
        }
    }
}

/// Remove team member
/// DELETE /api/bunker/team/:id
///
/// An id that is not a UUID is rejected with 400 by the `Path<Uuid>` extractor.
pub async fn remove_team_member(
    State(state): State<AppState>,
    who: Option<axum::Extension<crate::proxy_auth::Caller>>,
    Path(id): Path<Uuid>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    let (by, _) = crate::connections_api::caller(who);
    match state.db.remove_team_member_as(id, crate::audit::actor(&by)) {
        Ok(RemoveOutcome::Removed) => {
            info!("Removed team member: {}", id);
            Ok(Json(TeamOperationResponse {
                success: true,
                message: "Team member removed".to_string(),
            }))
        }
        Ok(RemoveOutcome::NotFound) => Err((StatusCode::NOT_FOUND, "Team member not found".to_string())),
        // Removing the only administrator would leave nobody able to administer the bunker (#25).
        Ok(RemoveOutcome::LastAdministrator) => Err((
            StatusCode::CONFLICT,
            "Cannot remove the last administrator".to_string(),
        )),
        Err(e) => {
            error!("Failed to remove team member {}: {}", id, e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))
        }
    }
}

/// Change a team member's role
/// PATCH /api/bunker/team/:id
///
/// One of the two roles of #143, as `add_team_member` takes them. Both sign, so a member keeps
/// their NIP-46 connections and tokens; the change is recorded with who made it, in one transaction
/// (`Database::change_member_role_as`). The only administrator is never demoted (409), for the
/// reason they are never removed (#99).
pub async fn change_team_member_role(
    State(state): State<AppState>,
    who: Option<axum::Extension<crate::proxy_auth::Caller>>,
    Path(id): Path<Uuid>,
    Json(request): Json<ChangeRoleRequest>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    let role: Role = request
        .role
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid role".to_string()))?;

    let member = match state.db.find_member_by_id(id) {
        Ok(Some(member)) => member,
        Ok(None) => return Err((StatusCode::NOT_FOUND, "Team member not found".to_string())),
        Err(e) => {
            error!("Failed to look up team member {}: {}", id, e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()));
        }
    };

    let (by, _) = crate::connections_api::caller(who);
    match state.db.change_member_role_as(&member.pubkey, role, crate::audit::actor(&by)) {
        Ok(RoleChangeOutcome::Changed) => {
            info!("Changed the role of team member {} to {}", id, role);
            Ok(Json(TeamOperationResponse {
                success: true,
                message: "Role changed".to_string(),
            }))
        }
        // Removed between the lookup and the change.
        Ok(RoleChangeOutcome::NotFound) => Err((StatusCode::NOT_FOUND, "Team member not found".to_string())),
        Ok(RoleChangeOutcome::LastAdministrator) => Err((
            StatusCode::CONFLICT,
            "Cannot demote the last administrator".to_string(),
        )),
        Err(e) => {
            error!("Failed to change the role of team member {}: {}", id, e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))
        }
    }
}

/// Creates the Axum router with all routes
pub fn create_router(state: AppState) -> Router {
    // Enforcement is on when BANCWR_PROXY_SECRET is set. main.rs refuses to start without it (#11),
    // so it is always on in a running bunker; tests can still build the router without it.
    // Nothing else holds the config lock while the router is built.
    let secret = state
        .config
        .try_read()
        .expect("config is not locked while the router is built")
        .proxy_secret
        .clone()
        .map(|secret| -> std::sync::Arc<[u8]> { secret.into_bytes().into() });

    // Each group of /api/bunker/* routes, behind the guard for who may call it (#25). One set of
    // used nonces for all of them, so a signed request cannot be sent twice to any route.
    let nonces = std::sync::Arc::new(Nonces::default());
    let guarded = |routes: Router<AppState>, access: Access| match &secret {
        Some(secret) => routes.route_layer(axum::middleware::from_fn_with_state(
            Guard { secret: secret.clone(), db: state.db.clone(), access, nonces: nonces.clone(), member_proof: state.member_proof.clone() },
            crate::proxy_auth::guard,
        )),
        None => routes,
    };

    // The access matrix (#25, #77, #143). A path may sit in two groups with different methods:
    // merging joins them, and each method keeps its own group's guard.
    let health = Router::new().route(STATUS_PATH, get(get_status));
    let administration = Router::new()
        // The signing log and audit trail (#96): paging, export and retention.
        .route("/api/bunker/logs", get(crate::logs_api::get_logs))
        .route("/api/bunker/logs/export", get(crate::logs_api::export_logs))
        .route("/api/bunker/logs/retention", get(crate::logs_api::get_retention).put(crate::logs_api::set_retention))
        .route("/api/bunker/metrics", get(get_metrics))
        // Read-only (#42): the signing key is encrypted at rest and set with `bunker import-key` (#94).
        .route("/api/bunker/config", get(get_config))
        // Reading the team and changing it, a role included (#99), are the administrator's (#77,
        // #143).
        .route("/api/bunker/team", get(get_team).post(add_team_member))
        .route("/api/bunker/team/:id", delete(remove_team_member).patch(change_team_member_role))
        // The bunker's own NIP-46 relays (#78): NIP46_RELAYS when set, otherwise the console's list.
        .route("/api/bunker/relays", get(crate::relays_api::get_relays).put(crate::relays_api::replace_relays))
        // Deciding requests held for approval (#102): administrators only.
        .route("/api/bunker/approvals/:id/approve", post(crate::approvals_api::approve))
        .route("/api/bunker/approvals/:id/reject", post(crate::approvals_api::reject))
        // The bunker's identities (#103): creating one with a key the bunker generates and retiring
        // one (#146), renaming one, and who may sign as it.
        .route("/api/bunker/identities", post(crate::identities_api::create_identity))
        .route("/api/bunker/identities/:pubkey", patch(crate::identities_api::rename_identity).delete(crate::identities_api::retire_identity))
        .route(
            "/api/bunker/identities/:pubkey/members/:member",
            put(crate::identities_api::grant_identity).delete(crate::identities_api::remove_identity_grant),
        )
        // An identity's profile, follows and relays, edited in the console (#150): signed as the
        // identity, for those kinds and a Blossom upload authorisation only, and logged.
        .route("/api/bunker/identities/:pubkey/sign", post(crate::identities_api::sign_as_identity));
    // Also the service identity's: sign-in looks the presented key up before any session exists.
    let lookup = Router::new().route("/api/bunker/team/by-pubkey/:pubkey", get(get_team_member_by_pubkey));
    // Administrators and signers, each scoped to the caller: their connection tokens (#142) and
    // connections, and revoking one (#53, #31; an administrator sees and revokes any), and their
    // own recent signatures (#77).
    let signer = Router::new()
        // NIP-46 connection tokens (#53), only for a member who can sign (#77). Administrators
        // issue for any such member, list and revoke all; signers only their own (#142).
        .route("/api/bunker/connections/tokens", post(crate::connections_api::issue_token).get(crate::connections_api::list_tokens))
        .route("/api/bunker/connections/tokens/:id", delete(crate::connections_api::revoke_token))
        .route("/api/bunker/connections", get(crate::connections_api::list_connections))
        // Connecting an app from its own nostrconnect:// code (#148): the same people, under the
        // same rules, as issuing a token.
        .route("/api/bunker/connections/nostrconnect/preview", post(crate::connections_api::preview_nostrconnect))
        .route("/api/bunker/connections/nostrconnect", post(crate::connections_api::connect_nostrconnect))
        .route("/api/bunker/connections/:id", delete(crate::connections_api::revoke_connection))
        .route("/api/bunker/logs/mine", get(get_my_logs))
        // Requests held for approval (#102): administrators see all, signers their own.
        .route("/api/bunker/approvals", get(crate::approvals_api::list_approvals))
        // The identities each may sign as (#103): administrators see all.
        .route("/api/bunker/identities", get(crate::identities_api::list_identities));

    // SPIKE (#151): starting a member's session on this bunker from their own sign-in.
    let member_proof = Router::new()
        .route("/api/bunker/auth/challenge", post(crate::member_proof_api::challenge))
        .route("/api/bunker/auth/proof", post(crate::member_proof_api::proof));

    // No CORS layer: browsers only ever call the frontend's Nitro server, which reaches the bunker
    // server-side. Without CORS headers, a browser on another site cannot call it. There is no
    // POST /sign either: it signed anything for anyone who could reach this port (#25). The only
    // route that signs on request is an administrator's, as an identity, for its profile, follows,
    // relays and image uploads (#150, `console_signing`).
    Router::new()
        .route(HEALTH_PATH, get(health_check))
        .merge(guarded(health, Access::Health))
        .merge(guarded(administration, Access::Administrator))
        .merge(guarded(lookup, Access::AdministratorOrService))
        .merge(guarded(signer, Access::Signer))
        .merge(guarded(member_proof, Access::Service))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// What a locked bunker knows (#94): why it is locked, and enough to answer the status.
#[derive(Clone)]
pub struct LockedState {
    pub reason: String,
    /// The public key the key file records, as an npub; empty when there is none to read.
    pub pubkey: String,
    pub version: String,
    pub nip46_enabled: bool,
    pub db: std::sync::Arc<crate::db::Database>,
}

/// The body every route but the status and the member lookup answers with while locked.
fn locked_body(reason: &str) -> Json<HealthResponse> {
    Json(HealthResponse { status: "locked".to_string(), reason: Some(reason.to_string()) })
}

/// GET /health while locked: 503, so the container is unhealthy, with the reason.
pub async fn locked_health(State(state): State<LockedState>) -> (StatusCode, Json<HealthResponse>) {
    (StatusCode::SERVICE_UNAVAILABLE, locked_body(&state.reason))
}

/// GET /api/bunker/status while locked: unhealthy, with the signer check saying why.
pub async fn locked_status(State(state): State<LockedState>) -> Json<BunkerStatus> {
    let checks = crate::health::locked_checks(&state.reason, state.nip46_enabled, &state.db);
    Json(BunkerStatus {
        status: crate::health::assess(&checks),
        pubkey: state.pubkey.clone(),
        // The identities recorded (#103), so sign-in still refuses each while locked.
        identities: state.db.list_identities().map(|all| all.into_iter().map(|i| i.pubkey).collect()).unwrap_or_default(),
        version: state.version.clone(),
        checks,
    })
}

async fn locked_lookup(
    State(state): State<LockedState>,
    Path(key): Path<String>,
) -> Result<Json<TeamMemberResponse>, (StatusCode, Json<serde_json::Value>)> {
    lookup_member(&state.db, &key)
}

async fn locked_fallback(State(state): State<LockedState>) -> (StatusCode, Json<HealthResponse>) {
    (StatusCode::SERVICE_UNAVAILABLE, locked_body(&state.reason))
}

/// The router while the bunker is locked (#94). Nothing here can sign. `/health` answers 503 with
/// the reason; the status and the member lookup that sign-in needs keep their guards, so an
/// administrator can sign in and read why; every other route answers 503.
pub fn create_locked_router(state: LockedState, proxy_secret: Option<String>) -> Router {
    let secret = proxy_secret.map(|secret| -> std::sync::Arc<[u8]> { secret.into_bytes().into() });
    // One set of used nonces for both guarded routes, as in `create_router`.
    let nonces = std::sync::Arc::new(Nonces::default());
    let guarded = |routes: Router<LockedState>, access: Access| match &secret {
        Some(secret) => routes.route_layer(axum::middleware::from_fn_with_state(
            Guard { secret: secret.clone(), db: state.db.clone(), access, nonces: nonces.clone(), member_proof: None },
            crate::proxy_auth::guard,
        )),
        None => routes,
    };
    let status = Router::new().route(STATUS_PATH, get(locked_status));
    let lookup = Router::new().route("/api/bunker/team/by-pubkey/:pubkey", get(locked_lookup));

    Router::new()
        .route(HEALTH_PATH, get(locked_health))
        .merge(guarded(status, Access::Health))
        .merge(guarded(lookup, Access::AdministratorOrService))
        .fallback(locked_fallback)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Graceful shutdown signal handler
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("Shutting down gracefully...");
}
