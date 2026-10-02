use crate::health::{Check, Overall};
use crate::registry::{canonical_pubkey, Role};
use crate::state::AppState;
use crate::db::RemoveOutcome;
use crate::proxy_auth::{Access, Guard};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
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
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BunkerStatus {
    /// Healthy, degraded or unhealthy, from the checks (#27).
    pub status: Overall,
    pub pubkey: String,
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
    pub pubkey: String,
    pub event_kind: u32,
    pub timestamp: String, // ISO 8601 format
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Metrics {
    pub http_requests: u64,
    pub nip46_connections: usize,
    pub total_signatures: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigResponse {
    pub pubkey: String,
    pub nsec_file: Option<String>,
    // Note: nsec is intentionally omitted for security
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TeamMemberResponse {
    pub id: String,
    pub name: String,
    /// Canonical lowercase hex (#24).
    pub pubkey: String,
    /// For display. `None` only for a row stored before #24 whose key is not valid.
    pub npub: Option<String>,
    /// `administrator`, `user` or `signer`; a row from before #24 may hold another value.
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
    })
}

/// Get signing logs
/// GET /api/bunker/logs
pub async fn get_logs(
    State(state): State<AppState>,
) -> Result<Json<Vec<LogEntry>>, (StatusCode, String)> {
    match state.db.get_recent_logs(100) {
        Ok(logs) => {
            let entries: Vec<LogEntry> = logs
                .into_iter()
                .map(|log| LogEntry {
                    id: log.id.to_string(),
                    event_id: log.event_id,
                    pubkey: log.pubkey,
                    event_kind: log.event_kind,
                    timestamp: log.timestamp.to_rfc3339(),
                })
                .collect();
            Ok(Json(entries))
        }
        Err(e) => {
            error!("Failed to fetch logs: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ))
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
        nsec_file: state.config.read().await.nsec_file.clone(),
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
    Json(request): Json<AddTeamMemberRequest>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    // The three roles settled in #24; the old `admin` and `viewer` are refused.
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
    if pubkey == state.signer.read().await.public_key_hex() {
        return Err((StatusCode::BAD_REQUEST, "The bunker's own key cannot be registered".to_string()));
    }

    match state.db.find_member_by_pubkey(&pubkey) {
        Ok(Some(_)) => return Err((StatusCode::CONFLICT, "This key is already registered".to_string())),
        Ok(None) => {}
        Err(e) => {
            error!("Failed to look up team member: {}", e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()));
        }
    }

    match state.db.add_team_member(&request.name, &pubkey, role) {
        Ok(_) => {
            info!("Added team member: {}", request.name);
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
    let pubkey = canonical_pubkey(&key).map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "invalid_pubkey" })))
    })?;
    match state.db.find_member_by_pubkey(&pubkey) {
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
    Path(id): Path<Uuid>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    match state.db.remove_team_member(id) {
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

    // Each group of /api/bunker/* routes, behind the guard for who may call it (#25).
    let guarded = |routes: Router<AppState>, access: Access| match &secret {
        Some(secret) => routes.route_layer(axum::middleware::from_fn_with_state(
            Guard { secret: secret.clone(), db: state.db.clone(), access },
            crate::proxy_auth::guard,
        )),
        None => routes,
    };

    let health = Router::new().route(STATUS_PATH, get(get_status));
    let administration = Router::new()
        .route("/api/bunker/logs", get(get_logs))
        .route("/api/bunker/metrics", get(get_metrics))
        // Read-only: the signing key is set with BUNKER_NSEC_FILE or BUNKER_NSEC (#42).
        .route("/api/bunker/config", get(get_config))
        .route("/api/bunker/team", get(get_team).post(add_team_member))
        .route("/api/bunker/team/:id", delete(remove_team_member));
    // Also the service identity's: sign-in looks the presented key up before any session exists.
    let lookup = Router::new().route("/api/bunker/team/by-pubkey/:pubkey", get(get_team_member_by_pubkey));
    // NIP-46 connections (#53): tokens and revocation for administrators; the list for every
    // member, scoped to the caller.
    let connections_admin = Router::new()
        .route("/api/bunker/connections/tokens", post(crate::connections_api::issue_token).get(crate::connections_api::list_tokens))
        .route("/api/bunker/connections/tokens/:id", delete(crate::connections_api::revoke_token));
    // Every member: the list, and revoking, both scoped to the caller (#31).
    let connections_member = Router::new()
        .route("/api/bunker/connections", get(crate::connections_api::list_connections))
        .route("/api/bunker/connections/:id", delete(crate::connections_api::revoke_connection));

    // No CORS layer: browsers only ever call the frontend's Nitro server, which reaches the bunker
    // server-side. Without CORS headers, a browser on another site cannot call it. There is no
    // POST /sign either: it signed anything for anyone who could reach this port (#25).
    Router::new()
        .route(HEALTH_PATH, get(health_check))
        .merge(guarded(health, Access::Health))
        .merge(guarded(administration, Access::Administrator))
        .merge(guarded(lookup, Access::AdministratorOrService))
        .merge(guarded(connections_admin, Access::Administrator))
        .merge(guarded(connections_member, Access::Member))
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
