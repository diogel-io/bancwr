//! The approval routes (#102), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `GET /api/bunker/approvals` | administrator and signer: administrators see all, signers their own, and a lead also their department's (#147) |
//! | `POST /api/bunker/approvals/:id/approve` | administrator, and a lead of the department whose identity the connection signs as (#147) |
//! | `POST /api/bunker/approvals/:id/reject` | administrator, and a lead of that department (#147) |
//!
//! A request waiting for approval shows what will be signed: its kind, content, tags, `created_at`
//! and event id. Approving names that event id, so an approver approves exactly what they saw.
//!
//! Who decides is amendment 1 to the signing approval ADR (departments ADR, #147): administrators,
//! and the leads of the department whose identity the connection signs as, their own requests
//! included. On an administrator only identity, administrators alone decide. Signers list their own
//! requests waiting, but don't decide them.
use crate::approvals::Pending;
use crate::connections_api::{api_error, caller, ApiError};
use crate::nip46::ApprovalError;
use crate::proxy_auth::Caller;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// How often requests that timed out, or whose connection ended, are answered.
pub const SWEEP_SECONDS: u64 = 15;

/// A request waiting for approval, as the console shows it.
#[derive(Serialize, Deserialize, Debug)]
pub struct ApprovalResponse {
    pub id: String,
    /// The member the connection is for (hex), and their name while they are in the vault.
    pub member_pubkey: String,
    pub member_name: Option<String>,
    pub connection_id: String,
    /// The app's NIP-46 key (hex), and the name it gave itself: never verified.
    pub client_pubkey: String,
    pub client_name: Option<String>,
    /// The identity it would be signed as (#103), hex. None: the primary.
    pub identity_pubkey: Option<String>,
    /// The event that will be signed, as the app sent it, under the bunker's key.
    pub kind: u16,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    /// Unix seconds, as the app set it.
    pub created_at: u64,
    pub event_id: String,
    pub requested_at: String,
    pub expires_at: String,
}

impl ApprovalResponse {
    fn of(pending: Pending, member_name: Option<String>) -> Self {
        Self {
            id: pending.id.to_string(),
            member_pubkey: pending.connection.for_pubkey.clone(),
            member_name,
            connection_id: pending.connection.id.clone(),
            client_pubkey: pending.connection.client_pubkey.clone(),
            client_name: pending.connection.client_name.clone(),
            identity_pubkey: pending.connection.identity_pubkey.clone(),
            kind: pending.kind(),
            content: pending.event.content.clone(),
            tags: pending.event.tags.iter().map(|tag| tag.as_slice().to_vec()).collect(),
            created_at: pending.event.created_at.as_u64(),
            event_id: pending.event_id.to_hex(),
            requested_at: pending.requested_at.to_rfc3339(),
            expires_at: pending.expires_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
pub struct ApproveRequest {
    /// The id of the event the administrator was shown.
    pub event_id: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct RejectRequest {
    #[serde(default)]
    pub note: Option<String>,
}

fn pending_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| not_found())
}

fn not_found() -> ApiError {
    api_error(StatusCode::NOT_FOUND, "not_found", "No request with that id is waiting for approval.")
}

/// Requests waiting for approval, oldest first: an administrator sees all, anyone else their own,
/// and a lead also those that would sign as their department's identity (#147).
/// GET /api/bunker/approvals
pub async fn list_approvals(State(state): State<AppState>, who: Option<Extension<Caller>>) -> Result<Json<Vec<ApprovalResponse>>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let mut pending = state.nip46_handler.pending_approvals(None);
    if !administrator {
        let led = state.db.led_identities(&pubkey).map_err(crate::connections_api::database_error)?;
        pending.retain(|request| {
            request.connection.for_pubkey == pubkey || led.iter().any(|key| key.as_str() == request.connection.identity_pubkey.as_deref().unwrap_or(""))
        });
    }
    let mut names = std::collections::HashMap::new();
    let mut listed = Vec::with_capacity(pending.len());
    for request in pending {
        let member = request.connection.for_pubkey.clone();
        if !names.contains_key(&member) {
            let name = state.db.find_member_by_pubkey(&member).ok().flatten().map(|m| m.name);
            names.insert(member.clone(), name);
        }
        listed.push(ApprovalResponse::of(request, names.get(&member).cloned().flatten()));
    }
    Ok(Json(listed))
}

/// Whether the caller may decide the waiting request `id` (#147): an administrator, or a lead of
/// the department whose identity its connection signs as, read from the database now. Anyone who
/// leads no department is a 403 before the request is looked up. A lead is refused someone else's
/// request outside their departments with a 404, as an unknown one is, and their own with a 403.
fn may_decide(state: &AppState, id: Uuid, pubkey: &str, administrator: bool) -> Result<(), ApiError> {
    if administrator {
        return Ok(());
    }
    let forbidden = || api_error(StatusCode::FORBIDDEN, "forbidden", "Only an administrator or a lead of this department can decide this request.");
    if !state.db.leads_any_department(pubkey).map_err(crate::connections_api::database_error)? {
        return Err(forbidden());
    }
    let waiting = state.nip46_handler.pending_approval(id).ok_or_else(not_found)?;
    if state.db.may_lead(pubkey, waiting.connection.identity_pubkey.as_deref()).map_err(crate::connections_api::database_error)? {
        return Ok(());
    }
    if waiting.connection.for_pubkey == pubkey {
        return Err(forbidden());
    }
    Err(not_found())
}

/// Approve a waiting request: the bunker signs the event and sends it to the app.
/// POST /api/bunker/approvals/:id/approve
pub async fn approve(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(id): Path<String>,
    Json(request): Json<ApproveRequest>,
) -> Result<Json<Value>, ApiError> {
    let id = pending_id(&id)?;
    let (actor, administrator) = caller(who);
    may_decide(&state, id, &actor, administrator)?;
    match state.nip46_handler.approve(id, &request.event_id, crate::audit::actor(&actor), request.note.as_deref()).await {
        Ok(reply) => {
            let signed = reply.response.result.is_some();
            crate::relay::deliver(&state, vec![reply]).await;
            if signed {
                Ok(Json(json!({ "success": true })))
            } else {
                Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "signing_error", "The event could not be signed; the app was told."))
            }
        }
        Err(ApprovalError::NotFound) => Err(not_found()),
        Err(ApprovalError::EventMismatch) => {
            Err(api_error(StatusCode::CONFLICT, "event_changed", "That is not the event waiting for approval. Reload and check it again."))
        }
        Err(ApprovalError::ConnectionEnded(reply)) => {
            crate::relay::deliver(&state, vec![*reply]).await;
            Err(api_error(StatusCode::CONFLICT, "connection_ended", "The app's connection has ended, so nothing was signed."))
        }
        Err(ApprovalError::Internal) => Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error")),
    }
}

/// Reject a waiting request: nothing is signed, and the app is told, with the note when given.
/// POST /api/bunker/approvals/:id/reject
pub async fn reject(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(id): Path<String>,
    request: Option<Json<RejectRequest>>,
) -> Result<Json<Value>, ApiError> {
    let id = pending_id(&id)?;
    let (actor, administrator) = caller(who);
    may_decide(&state, id, &actor, administrator)?;
    let request = request.map(|Json(r)| r).unwrap_or_default();
    match state.nip46_handler.reject(id, crate::audit::actor(&actor), request.note.as_deref()) {
        Ok(reply) => {
            crate::relay::deliver(&state, vec![reply]).await;
            Ok(Json(json!({ "success": true })))
        }
        Err(ApprovalError::NotFound) => Err(not_found()),
        Err(_) => Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error")),
    }
}

/// Answers every request that timed out or whose connection ended (#102). Run every
/// `SWEEP_SECONDS` by `main.rs`.
pub async fn sweep(state: &AppState) {
    let replies = state.nip46_handler.sweep_approvals(Utc::now());
    crate::relay::deliver(state, replies).await;
}
