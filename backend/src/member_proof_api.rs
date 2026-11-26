//! SPIKE (#151): the two routes that start a member's bunker session from their own sign-in. Both
//! are the service identity's: the console calls them before the member has a session here. See
//! `member_proof`.

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::registry::npub;
use crate::state::AppState;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn refuse(status: StatusCode, body: Value) -> (StatusCode, Json<Value>) {
    (status, Json(body))
}

fn not_enabled() -> (StatusCode, Json<Value>) {
    refuse(StatusCode::NOT_FOUND, json!({ "error": "member_proof_off" }))
}

/// POST /api/bunker/auth/challenge: a single-use challenge for the next sign-in, 300 s.
pub async fn challenge(State(state): State<AppState>) -> ApiResult {
    let proof = state.member_proof.as_ref().ok_or_else(not_enabled)?;
    let now = chrono::Utc::now().timestamp();
    Ok(Json(json!({ "challenge": proof.issue_challenge(now) })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofRequest {
    /// The `Authorization` header the console received, `Nostr <base64 event>`.
    authorization: String,
    /// The exact login body the console received: its SHA-256 is the event's `payload` tag.
    body: String,
}

/// POST /api/bunker/auth/proof: checks the member's NIP-98 sign-in and starts a session on this
/// bunker. 401 for an event that fails a rule or carries none of this bunker's challenges; 403
/// `bunker_key` for one of this bunker's identities (rule 10); 403 `not_registered` for a key not
/// in this bunker's team.
pub async fn proof(State(state): State<AppState>, Json(request): Json<ProofRequest>) -> ApiResult {
    let proof = state.member_proof.as_ref().ok_or_else(not_enabled)?;
    let now = chrono::Utc::now().timestamp();
    let pubkey = proof
        .verify(&request.authorization, &request.body, now)
        .map_err(|e| refuse(StatusCode::UNAUTHORIZED, json!({ "error": "not_authenticated", "reason": e.reason() })))?;

    let database_error = |e: anyhow::Error| {
        tracing::error!("member proof: {}", e);
        refuse(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "database_error" }))
    };
    let identities = state.db.list_identities().map_err(database_error)?;
    if identities.iter().any(|i| i.pubkey == pubkey) {
        return Err(refuse(StatusCode::FORBIDDEN, json!({ "error": "forbidden", "reason": "bunker_key" })));
    }
    let member = state.db.find_member_by_pubkey(&pubkey).map_err(database_error)?;
    let Some(member) = member else {
        return Err(refuse(StatusCode::FORBIDDEN, json!({ "error": "not_registered", "npub": npub(&pubkey) })));
    };

    let (session, expires_at) = proof.start_session(&pubkey, now);
    tracing::info!("Started a bunker session for {} from their own sign-in", npub(&pubkey).unwrap_or_default());
    Ok(Json(json!({ "session": session, "expires_at": expires_at, "pubkey": pubkey, "role": member.role })))
}
