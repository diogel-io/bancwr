//! The bunker's own relays (#78), for administrators, behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `GET /api/bunker/relays` | administrator |
//! | `PUT /api/bunker/relays` | administrator |
//!
//! `GET` returns the relays in force, where they come from and whether each is connected. `PUT`
//! replaces the list saved in the console, whole, and applies it to the running relay client at
//! once. While `NIP46_RELAYS` is set it decides, and `PUT` is refused with 409
//! `relays_from_environment` (see `bunker_relays` for the rule).
use crate::bunker_relays::{self, RelaySource};
use crate::connections_api::{api_error, caller, ApiError};
use crate::health::{connected_relays, RelayHealth};
use crate::proxy_auth::Caller;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, Extension, Json};
use chrono::Utc;
use nostr::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Serialize, Deserialize, Debug)]
pub struct RelaysResponse {
    /// `environment` (NIP46_RELAYS, read-only) or `console`.
    pub source: RelaySource,
    /// Whether NIP-46 is on (NIP46_ENABLED). While it is, a saved list needs at least one relay.
    pub nip46_enabled: bool,
    /// In order: the order of the relays in a `bunker://` string.
    pub relays: Vec<RelayHealth>,
}

#[derive(Deserialize)]
pub struct ReplaceRelaysRequest {
    pub relays: Vec<String>,
}

fn database_error(e: anyhow::Error) -> ApiError {
    error!("Database error in the relay routes: {}", e);
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error")
}

async fn current(state: &AppState) -> Result<RelaysResponse, ApiError> {
    let nip46_enabled = state.config.read().await.nip46_enabled;
    let relays = bunker_relays::effective_relays(state).await.map_err(database_error)?;
    let connected = connected_relays(state).await;
    Ok(RelaysResponse {
        source: relays.source,
        nip46_enabled,
        relays: relays
            .urls
            .into_iter()
            .map(|url| RelayHealth {
                connected: RelayUrl::parse(&url).map(|parsed| connected.contains(&parsed)).unwrap_or(false),
                url,
            })
            .collect(),
    })
}

/// The bunker's relays
/// GET /api/bunker/relays
pub async fn get_relays(State(state): State<AppState>) -> Result<Json<RelaysResponse>, ApiError> {
    Ok(Json(current(&state).await?))
}

/// Replace the bunker's relays
/// PUT /api/bunker/relays
pub async fn replace_relays(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Json(request): Json<ReplaceRelaysRequest>,
) -> Result<Json<RelaysResponse>, ApiError> {
    let (by, _) = caller(who);
    // One save at a time, stored and applied together, so the running client always ends on the
    // list stored last.
    let _update = state.relays_update.lock().await;
    let (nip46_enabled, from_environment) = {
        let config = state.config.read().await;
        (config.nip46_enabled, !config.relay_urls.is_empty())
    };
    if from_environment {
        return Err(api_error(
            StatusCode::CONFLICT,
            "relays_from_environment",
            "NIP46_RELAYS is set, so it decides the bunker's relays. Unset it and restart the bunker to manage them here.",
        ));
    }
    let urls = bunker_relays::validate(&request.relays, nip46_enabled)
        .map_err(|refusal| api_error(StatusCode::BAD_REQUEST, refusal.code(), &refusal.message()))?;

    state.db.replace_bunker_relays(&urls, &by, Utc::now()).map_err(database_error)?;
    info!("Bunker relays set to [{}] (by {})", urls.join(", "), if by.is_empty() { "-" } else { &by });

    // Applied to the running client at once. Stored either way: with NIP-46 off there is no
    // client, and the list is used when it is turned on.
    if nip46_enabled {
        match state.relay_handle().await {
            Some(handle) => {
                if let Err(e) = handle.set_relays(&urls).await {
                    // Stored, so a restart applies it; the health check shows what is connected.
                    error!("The bunker relays were saved but not all applied: {}", e);
                }
            }
            None => info!("The NIP-46 relay client has not started; it uses the saved relays when it does"),
        }
    }

    Ok(Json(current(&state).await?))
}
