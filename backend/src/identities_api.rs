//! The identity routes (#103), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `GET /api/bunker/identities` | administrator and signer: administrators see all with their members, signers those they may use |
//! | `PATCH /api/bunker/identities/:pubkey` | administrator |
//! | `PUT /api/bunker/identities/:pubkey/members/:member` | administrator |
//! | `DELETE /api/bunker/identities/:pubkey/members/:member` | administrator |
//!
//! Identities are added at the bunker's command line (`bunker import-key --add`), never here, so a
//! key never passes through the browser. Removing one is not supported yet. A member's own key
//! (#138, spike: `--add --owner`) is listed with its owner, is never granted, and only its owner
//! may use it.
use crate::connections_api::{api_error, caller, database_error, ApiError};
use crate::db::{Identity, MAX_IDENTITY_LABEL_CHARS};
use crate::proxy_auth::Caller;
use crate::registry::canonical_pubkey;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

/// One identity, never its key.
#[derive(Serialize, Deserialize, Debug)]
pub struct IdentityResponse {
    /// Hex.
    pub pubkey: String,
    pub npub: Option<String>,
    pub label: String,
    pub is_primary: bool,
    /// Whether it is unlocked in the running bunker: one added since it started loads on restart.
    pub available: bool,
    /// The members granted it, hex: for administrators only. Administrators may use every
    /// identity without a grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<String>>,
    /// The member whose own key it is (#138), hex; absent for the bunker's own identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Deserialize)]
pub struct RenameIdentityRequest {
    pub label: String,
}

async fn response(state: &AppState, identity: Identity, members: Option<Vec<String>>) -> IdentityResponse {
    let available = state.keyring.is_identity(&identity.pubkey).await;
    IdentityResponse {
        npub: crate::registry::npub(&identity.pubkey),
        pubkey: identity.pubkey,
        label: identity.label,
        is_primary: identity.is_primary,
        available,
        members,
        owner: identity.owner_pubkey,
    }
}

fn not_found() -> ApiError {
    api_error(StatusCode::NOT_FOUND, "not_found", "No identity with that key.")
}

/// The identity a path names, as hex or npub.
fn find(state: &AppState, key: &str) -> Result<Identity, ApiError> {
    let hex = canonical_pubkey(key).map_err(|_| not_found())?;
    state.db.find_identity(&hex).map_err(database_error)?.ok_or_else(not_found)
}

/// Every identity for an administrator, with its members; those the caller may use otherwise.
/// GET /api/bunker/identities
pub async fn list_identities(State(state): State<AppState>, who: Option<Extension<Caller>>) -> Result<Json<Vec<IdentityResponse>>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let mut listed = Vec::new();
    for identity in state.db.list_identities().map_err(database_error)? {
        let reference = identity.reference().map(str::to_string);
        if administrator {
            let members = state.db.identity_members(reference.as_deref()).map_err(database_error)?;
            listed.push(response(&state, identity, Some(members)).await);
        } else if state.db.may_use_identity(&pubkey, reference.as_deref()).map_err(database_error)? {
            listed.push(response(&state, identity, None).await);
        }
    }
    Ok(Json(listed))
}

/// Rename an identity. PATCH /api/bunker/identities/:pubkey
pub async fn rename_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(key): Path<String>,
    Json(request): Json<RenameIdentityRequest>,
) -> Result<Json<Value>, ApiError> {
    let identity = find(&state, &key)?;
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > MAX_IDENTITY_LABEL_CHARS {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_label", "Give the identity a label of up to 100 characters."));
    }
    let (by, _) = caller(who);
    state.db.rename_identity(&identity.pubkey, label, crate::audit::actor(&by), Utc::now()).map_err(database_error)?;
    info!("Renamed identity {}", identity.pubkey);
    Ok(Json(json!({ "success": true })))
}

/// The member a path names, who must be in the vault and able to sign.
fn signing_member(state: &AppState, key: &str) -> Result<String, ApiError> {
    let hex = canonical_pubkey(key).map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "That is not a valid npub or hex key."))?;
    match state.db.find_member_by_pubkey(&hex).map_err(database_error)? {
        Some(member) if member.role().is_some_and(|role| role.can_sign()) => Ok(hex),
        Some(_) => Err(api_error(StatusCode::BAD_REQUEST, "member_cannot_sign", "Only an admin or a signer can be granted an identity, not a viewer.")),
        None => Err(api_error(StatusCode::NOT_FOUND, "not_registered", "That key is not a member of the vault.")),
    }
}

/// Grant a member an identity. PUT /api/bunker/identities/:pubkey/members/:member
pub async fn grant_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path((key, member)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let identity = find(&state, &key)?;
    if identity.owner_pubkey.is_some() {
        return Err(api_error(StatusCode::BAD_REQUEST, "owned_identity", "That is a member's own key. Nobody else can be granted it."));
    }
    let member = signing_member(&state, &member)?;
    let (by, _) = caller(who);
    let added = state.db.grant_identity(identity.reference(), &member, crate::audit::actor(&by), Utc::now()).map_err(database_error)?;
    if added {
        info!("Granted {} the identity {}", member, identity.pubkey);
    }
    Ok(Json(json!({ "success": true })))
}

/// Remove a member's grant of an identity, revoking their connections and unused tokens for it.
/// DELETE /api/bunker/identities/:pubkey/members/:member
pub async fn remove_identity_grant(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path((key, member)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let identity = find(&state, &key)?;
    let member = canonical_pubkey(&member).map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "That is not a valid npub or hex key."))?;
    let (by, _) = caller(who);
    match state.db.remove_identity_grant(identity.reference(), &member, crate::audit::actor(&by), Utc::now()).map_err(database_error)? {
        Some((connections, tokens)) => {
            info!("Removed {}'s grant of {}: {} connections and {} tokens revoked", member, identity.pubkey, connections, tokens);
            Ok(Json(json!({ "success": true, "connections_revoked": connections, "tokens_revoked": tokens })))
        }
        None => Err(api_error(StatusCode::NOT_FOUND, "not_found", "That member is not granted that identity.")),
    }
}
