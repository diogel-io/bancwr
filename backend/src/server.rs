use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use nostr::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use uuid::Uuid;

const HEALTH_PATH: &str = "/health";
const STATUS_PATH: &str = "/api/bunker/status";
const SIGN_PATH: &str = "/sign";

// Removed local AppState struct as it is now in state.rs
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BunkerStatus {
    pub status: String,
    pub pubkey: String,
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
pub struct ConfigUpdateRequest {
    pub nsec: Option<String>,
    pub nsec_file: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigUpdateResponse {
    pub success: bool,
    pub message: String,
    pub pubkey: Option<String>, // New pubkey if nsec was updated
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TeamMemberResponse {
    pub id: String,
    pub name: String,
    pub pubkey: String,
    pub role: String,
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
    Json(BunkerStatus {
        status: "healthy".to_string(),
        pubkey: state.signer.read().await.public_key_bech32(),
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

/// Sign event endpoint
/// POST /sign
pub async fn sign_event(
    State(state): State<AppState>,
    Json(unsigned_event): Json<UnsignedEvent>,
) -> Result<Json<Event>, (StatusCode, String)> {
    info!("Received sign request for event kind: {}", unsigned_event.kind);

    // Basic validation
    if unsigned_event.content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Content cannot be empty".to_string()));
    }
    
    if unsigned_event.kind == Kind::from(0) {
        return Err((StatusCode::BAD_REQUEST, "Kind 0 not allowed via this bunker".to_string()));
    }

    let event = state.signer
        .read()
        .await
        .sign_event(unsigned_event.clone())
        .await
        .map_err(|e| {
            error!("Signing failed: {}", e);
            (StatusCode::BAD_REQUEST, e.to_string())
        })?;

    // Increment request count in state
    state.increment_http_request_count();

    // Log the signing event to DuckDB
    if let Err(e) = state.db.log_signing_event(
        &event.id.to_hex(),
        &event.pubkey.to_bech32().unwrap_or_else(|_| event.pubkey.to_hex()),
        event.kind.as_u16() as u32,
        Utc::now(),
    ) {
        error!("Failed to log signing event to database: {}", e);
    }

    Ok(Json(event))
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

/// Update configuration
/// POST /api/bunker/config
pub async fn update_config(
    State(state): State<AppState>,
    Json(request): Json<ConfigUpdateRequest>,
) -> Result<Json<ConfigUpdateResponse>, (StatusCode, String)> {
    // Validate: can't have both nsec and nsec_file
    if request.nsec.is_some() && request.nsec_file.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot specify both nsec and nsec_file".to_string()
        ));
    }
    
    // Handle nsec update
    if let Some(nsec_str) = request.nsec {
        // Parse nsec (try bech32 first, then hex)
        let secret_key = parse_nsec(&nsec_str)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid nsec: {}", e)))?;
        
        // Store in database config
        state.db.set_config("nsec", &nsec_str)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        
        let new_pubkey = Keys::new(secret_key).public_key().to_bech32()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        
        return Ok(Json(ConfigUpdateResponse {
            success: true,
            message: "Configuration updated. Restart required to apply new nsec.".to_string(),
            pubkey: Some(new_pubkey),
        }));
    }
    
    // Handle nsec_file update
    if let Some(nsec_file) = request.nsec_file {
        // Validate file exists
        if !std::path::Path::new(&nsec_file).exists() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("File not found: {}", nsec_file)
            ));
        }
        
        // Store in database config
        state.db.set_config("nsec_file", &nsec_file)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        
        return Ok(Json(ConfigUpdateResponse {
            success: true,
            message: "Configuration updated. Restart required to apply.".to_string(),
            pubkey: None,
        }));
    }
    
    // Nothing to update
    Ok(Json(ConfigUpdateResponse {
        success: true,
        message: "No changes made".to_string(),
        pubkey: None,
    }))
}

/// Parse nsec from bech32 or hex string
fn parse_nsec(nsec_str: &str) -> anyhow::Result<SecretKey> {
    if nsec_str.starts_with("nsec1") {
        SecretKey::from_bech32(nsec_str)
            .map_err(|e| anyhow::anyhow!("Invalid nsec: {}", e))
    } else {
        SecretKey::parse(nsec_str)
            .map_err(|e| anyhow::anyhow!("Invalid secret key: {}", e))
    }
}

/// Get team members
/// GET /api/bunker/team
pub async fn get_team(
    State(state): State<AppState>,
) -> Result<Json<Vec<TeamMemberResponse>>, (StatusCode, String)> {
    match state.db.get_team_members() {
        Ok(members) => {
            let response: Vec<TeamMemberResponse> = members
                .into_iter()
                .map(|m| TeamMemberResponse {
                    id: m.id.to_string(),
                    name: m.name,
                    pubkey: m.pubkey,
                    role: m.role,
                })
                .collect();
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
    // Validate role
    let valid_roles = ["admin", "signer", "viewer"];
    if !valid_roles.contains(&request.role.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "Invalid role".to_string()));
    }

    // Validate pubkey format (should be npub1...)
    if !request.pubkey.starts_with("npub1") {
        return Err((StatusCode::BAD_REQUEST, "Invalid pubkey format".to_string()));
    }

    match state.db.add_team_member(&request.name, &request.pubkey, &request.role) {
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

/// Remove team member
/// DELETE /api/bunker/team/:id
///
/// An id that is not a UUID is rejected with 400 by the `Path<Uuid>` extractor.
pub async fn remove_team_member(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TeamOperationResponse>, (StatusCode, String)> {
    match state.db.remove_team_member(id) {
        Ok(true) => {
            info!("Removed team member: {}", id);
            Ok(Json(TeamOperationResponse {
                success: true,
                message: "Team member removed".to_string(),
            }))
        }
        Ok(false) => Err((StatusCode::NOT_FOUND, "Team member not found".to_string())),
        Err(e) => {
            error!("Failed to remove team member {}: {}", id, e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))
        }
    }
}

/// Creates the Axum router with all routes
pub fn create_router(state: AppState) -> Router {
    // DELETE is deliberately not allowed cross-origin. The frontend reaches this API through its
    // own server-side proxy, which CORS does not apply to, and while the API is unauthenticated
    // (#25) a browser on another site must not be able to remove team members.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    Router::new()
        .route(HEALTH_PATH, get(health_check))
        .route(STATUS_PATH, get(get_status))
        .route("/api/bunker/logs", get(get_logs))
        .route("/api/bunker/metrics", get(get_metrics))
        .route("/api/bunker/config", get(get_config).post(update_config))
        .route("/api/bunker/team", get(get_team).post(add_team_member))
        .route("/api/bunker/team/:id", delete(remove_team_member))
        .route(SIGN_PATH, post(sign_event))
        .layer(cors)
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
