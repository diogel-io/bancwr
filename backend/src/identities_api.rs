//! The identity routes (#103), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `GET /api/bunker/identities` | administrator and signer: administrators see all with their members, signers those they may use |
//! | `POST /api/bunker/identities` | administrator |
//! | `PATCH /api/bunker/identities/:pubkey` | administrator |
//! | `DELETE /api/bunker/identities/:pubkey` | administrator |
//! | `PUT /api/bunker/identities/:pubkey/members/:member` | administrator |
//! | `DELETE /api/bunker/identities/:pubkey/members/:member` | administrator |
//! | `POST /api/bunker/identities/:pubkey/sign` | administrator |
//!
//! An administrator creates an identity here (#146): the bunker generates its key, encrypts it
//! under the root credential as `bunker import-key --add` does, and unlocks it at once. The key is
//! never sent, returned or logged, so it never passes through the browser. An identity imported at
//! the command line still loads at the next start. Retiring one revokes everything that signs as
//! it and destroys its key; its signing log and audit events stay. The primary is never retired.
//!
//! Who may create and retire is decided in one place, `may_manage_identities`, so departments
//! (#147) can widen it.
//!
//! An administrator edits an identity's profile, follows and relays in the console (#150): the
//! console builds each event and `sign_as_identity` signs it as the identity, for the four kinds
//! `console_signing` allows and nothing else. Each signature is logged under the administrator and
//! marked as the console's. Who may do so is decided in `may_edit_identity`, which departments
//! (#147) widen for a lead.
use crate::connections_api::{api_error, caller, database_error, ApiError};
use crate::console_signing::{self, EventTemplate};
use crate::db::{Identity, RetireOutcome, MAX_IDENTITY_LABEL_CHARS};
use crate::proxy_auth::Caller;
use crate::registry::canonical_pubkey;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{error, info, warn};

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
    /// What retiring it would revoke (#146): for administrators, on identities that can be retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<IdentityUsage>,
    /// When it was retired (#146): listed only with `?retired=true`, for administrators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<DateTime<Utc>>,
}

/// The active connections and unused tokens that sign as an identity.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityUsage {
    pub connections: usize,
    pub tokens: usize,
}

#[derive(Deserialize, Default)]
pub struct ListIdentitiesQuery {
    /// Include retired identities (#146), for administrators: so the logs can still name them.
    #[serde(default)]
    pub retired: bool,
}

#[derive(Deserialize)]
pub struct CreateIdentityRequest {
    pub label: String,
}

#[derive(Deserialize)]
pub struct RenameIdentityRequest {
    pub label: String,
}

async fn response(state: &AppState, identity: Identity, members: Option<Vec<String>>, usage: Option<IdentityUsage>) -> IdentityResponse {
    let available = identity.retired_at.is_none() && state.keyring.is_identity(&identity.pubkey).await;
    IdentityResponse {
        npub: crate::registry::npub(&identity.pubkey),
        pubkey: identity.pubkey,
        label: identity.label,
        is_primary: identity.is_primary,
        available,
        members,
        usage,
        retired_at: identity.retired_at,
    }
}

fn not_found() -> ApiError {
    api_error(StatusCode::NOT_FOUND, "not_found", "No identity with that key.")
}

/// The identity a path names, as hex or npub. A retired one is not found (#146): it can no longer
/// be renamed or granted.
fn find(state: &AppState, key: &str) -> Result<Identity, ApiError> {
    let hex = canonical_pubkey(key).map_err(|_| not_found())?;
    state.db.find_identity(&hex).map_err(database_error)?.filter(|identity| identity.retired_at.is_none()).ok_or_else(not_found)
}

/// Who may create and retire identities (#146): administrators. The one place departments (#147)
/// widen.
fn may_manage_identities(administrator: bool) -> Result<(), ApiError> {
    if administrator {
        Ok(())
    } else {
        Err(api_error(StatusCode::FORBIDDEN, "forbidden", "Only an administrator can create or retire identities."))
    }
}

/// Every identity for an administrator, with its members; those the caller may use otherwise.
/// Retired identities only with `?retired=true`, and only for an administrator (#146).
/// GET /api/bunker/identities
pub async fn list_identities(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    query: Option<Query<ListIdentitiesQuery>>,
) -> Result<Json<Vec<IdentityResponse>>, ApiError> {
    let (pubkey, administrator) = caller(who);
    let include_retired = administrator && query.map(|Query(q)| q.retired).unwrap_or(false);
    let mut listed = Vec::new();
    for identity in state.db.list_identities().map_err(database_error)? {
        if identity.retired_at.is_some() {
            if include_retired {
                listed.push(response(&state, identity, None, None).await);
            }
            continue;
        }
        let reference = identity.reference().map(str::to_string);
        if administrator {
            let members = state.db.identity_members(reference.as_deref()).map_err(database_error)?;
            let usage = match reference.as_deref() {
                Some(hex) => {
                    let (connections, tokens) = state.db.identity_usage(hex).map_err(database_error)?;
                    Some(IdentityUsage { connections, tokens })
                }
                None => None,
            };
            listed.push(response(&state, identity, Some(members), usage).await);
        } else if state.db.may_use_identity(&pubkey, reference.as_deref()).map_err(database_error)? {
            listed.push(response(&state, identity, None, None).await);
        }
    }
    Ok(Json(listed))
}

/// Create an identity whose key the bunker generates (#146). POST /api/bunker/identities
///
/// The root credential is read as at start; if it cannot be, nothing changes. The new key is
/// encrypted under it and decrypted back once to check it before anything is stored, refused if it
/// is already an identity, a member's key or `BANCWR_ADMIN_PUBKEY`, then stored, unlocked and
/// listened for. The response names it by its public key only.
pub async fn create_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<IdentityResponse>), ApiError> {
    let (by, administrator) = caller(who);
    may_manage_identities(administrator)?;
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > MAX_IDENTITY_LABEL_CHARS {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_label", "Give the identity a label of up to 100 characters."));
    }
    let root = state.root_key().map_err(|e| {
        error!("Could not create an identity: the root credential could not be read ({})", e);
        api_error(StatusCode::SERVICE_UNAVAILABLE, "root_key_unavailable", e.reason())
    })?;
    let keys = nostr::Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let key_file = crate::key_store::encrypt(&root, keys.secret_key());
    // Checked before it is stored, as `bunker import-key` checks what it writes.
    let secret_key = crate::key_store::decrypt_identity(&root, &pubkey, &key_file).map_err(|e| {
        error!("Could not create an identity: its encrypted key did not decrypt back ({})", e);
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "key_check_failed", "The new key could not be checked, so nothing was stored.")
    })?;
    drop(root);
    drop(keys);
    if state.config.read().await.check_admin_pubkey(&pubkey).is_err() || state.keyring.is_identity(&pubkey).await {
        return Err(api_error(StatusCode::CONFLICT, "key_in_use", "The generated key is already in use. Try again."));
    }
    match state.db.add_identity_as(&pubkey, label, &key_file, crate::audit::actor(&by), Utc::now()).map_err(database_error)? {
        Ok(()) => {}
        Err(_) => return Err(api_error(StatusCode::CONFLICT, "key_in_use", "The generated key is already in use. Try again.")),
    }
    state.keyring.add(crate::signer::Signer::new(secret_key)).await;
    if let Err(e) = state.identities_changed().await {
        warn!("Created identity {}, but the NIP-46 subscription could not be updated yet: {}", pubkey, e);
    }
    info!("Created identity {}", pubkey);
    let identity = state.db.find_identity(&pubkey).map_err(database_error)?.ok_or_else(not_found)?;
    let usage = Some(IdentityUsage { connections: 0, tokens: 0 });
    Ok((StatusCode::CREATED, Json(response(&state, identity, Some(Vec::new()), usage).await)))
}

/// Retire an identity (#146): revoke every connection and unused token that signs as it, remove
/// its grants, destroy its encrypted key, and stop answering requests to it. Its signing log and
/// audit events stay. The primary is never retired. DELETE /api/bunker/identities/:pubkey
pub async fn retire_identity(State(state): State<AppState>, who: Option<Extension<Caller>>, Path(key): Path<String>) -> Result<Json<Value>, ApiError> {
    let (by, administrator) = caller(who);
    may_manage_identities(administrator)?;
    let hex = canonical_pubkey(&key).map_err(|_| not_found())?;
    match state.db.retire_identity(&hex, crate::audit::actor(&by), Utc::now()).map_err(database_error)? {
        RetireOutcome::Retired { connections, tokens } => {
            state.keyring.remove(&hex);
            if let Err(e) = state.identities_changed().await {
                warn!("Retired identity {}, but the NIP-46 subscription could not be updated yet: {}", hex, e);
            }
            info!("Retired identity {}: {} connections and {} tokens revoked", hex, connections, tokens);
            Ok(Json(json!({ "success": true, "connections_revoked": connections, "tokens_revoked": tokens })))
        }
        RetireOutcome::Primary => Err(api_error(StatusCode::BAD_REQUEST, "primary_identity", "The primary identity can't be retired.")),
        RetireOutcome::NotFound | RetireOutcome::AlreadyRetired => Err(not_found()),
    }
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
        Some(member) if member.role().is_some() => Ok(hex),
        Some(_) => Err(api_error(StatusCode::BAD_REQUEST, "member_cannot_sign", "This member has no recognised role and cannot sign.")),
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

/// Who may have an identity sign its profile, follows and relays (#150): administrators. The one
/// place departments (#147) widen, for a lead and their department's identity.
fn may_edit_identity(administrator: bool) -> Result<(), ApiError> {
    if administrator {
        Ok(())
    } else {
        Err(api_error(StatusCode::FORBIDDEN, "forbidden", "Only an administrator can edit an identity's profile."))
    }
}

/// Sign an event the console built as an identity (#150): its profile (kind 0), follow list
/// (kind 3), relay list (kind 10002), or a Blossom upload authorisation (kind 24242) for its
/// profile image. `console_signing::check` refuses anything else, and a refusal signs and logs
/// nothing. The identity must be this bunker's, not retired and unlocked. Each signature is logged
/// with the administrator as the member and marked as the console's.
/// POST /api/bunker/identities/:pubkey/sign
pub async fn sign_as_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(key): Path<String>,
    Json(template): Json<EventTemplate>,
) -> Result<Json<nostr::Event>, ApiError> {
    let (by, administrator) = caller(who);
    may_edit_identity(administrator)?;
    let identity = find(&state, &key)?;
    let signed = {
        let Some(signer) = state.keyring.signer(identity.reference()).await else {
            return Err(api_error(
                StatusCode::CONFLICT,
                "identity_unavailable",
                "This identity is not unlocked in the running bunker. Restart the bunker and try again.",
            ));
        };
        let unsigned = console_signing::check(&template, signer.public_key(), Utc::now().timestamp())
            .map_err(|refusal| api_error(StatusCode::BAD_REQUEST, refusal.code, &refusal.message))?;
        signer.sign_event(unsigned).await.map_err(|e| {
            error!("Could not sign a console edit as {}: {}", identity.pubkey, e);
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "signing_failed", "The bunker could not sign the event.")
        })?
    };
    // Logged before it is returned: a signature the log can't record is not handed out.
    state
        .db
        .log_console_signature(&signed.id.to_hex(), &by, identity.reference(), u32::from(signed.kind.as_u16()), Utc::now())
        .map_err(database_error)?;
    info!("Signed kind {} as identity {} for the console", signed.kind.as_u16(), identity.pubkey);
    Ok(Json(signed))
}
