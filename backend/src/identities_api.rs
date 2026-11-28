//! The identity routes (#103), behind #25's guard:
//!
//! | Route | Who |
//! |-------|-----|
//! | `GET /api/bunker/identities` | administrator and signer: administrators see all with their members and leads, signers those they may use, a lead with their department's members and leads |
//! | `POST /api/bunker/identities` | administrator, and a lead of any department, who leads the new one (#147) |
//! | `PATCH /api/bunker/identities/:pubkey` | administrator; a lead of its department may rename it (#147); only an administrator marks it administrator only |
//! | `DELETE /api/bunker/identities/:pubkey` | administrator, and a lead of its department (#147) |
//! | `PUT /api/bunker/identities/:pubkey/members/:member` | administrator; a lead of its department grants it as a `member` (#147); only an administrator appoints a lead |
//! | `DELETE /api/bunker/identities/:pubkey/members/:member` | administrator; a lead of its department removes a `member`, or themselves (#147) |
//! | `POST /api/bunker/identities/:pubkey/sign` | administrator, and a lead of its department (#147) |
//!
//! Departments (#147, departments ADR): each identity that is not administrator only is one
//! department, its members are the members granted it, and a grant's `role` is `member` or `lead`.
//! A lead's powers apply only to identities they hold a `lead` grant on, checked here on every
//! request with `Database::may_lead`, never from the session. An administrator only identity has
//! no members or leads, refuses grants, and only administrators reach it.
//!
//! An administrator creates an identity here (#146): the bunker generates its key, encrypts it
//! under the root credential as `bunker import-key --add` does, and unlocks it at once. The key is
//! never sent, returned or logged, so it never passes through the browser. An identity imported at
//! the command line still loads at the next start. Retiring one revokes everything that signs as
//! it and destroys its key; its signing log and audit events stay. The primary is never retired.
//!
//! Who may create and retire is decided in `may_create` and `may_manage`.
//!
//! An administrator edits an identity's profile, follows and relays in the console (#150): the
//! console builds each event and `sign_as_identity` signs it as the identity, for the four kinds
//! `console_signing` allows and nothing else. Each signature is logged under the administrator and
//! marked as the console's. Who may do so is decided in `may_manage`, as for a lead of its
//! department (#147).
use crate::connections_api::{api_error, caller, database_error, ApiError};
use crate::console_signing::{self, EventTemplate};
use crate::db::{AddIdentityRefusal, GrantOutcome, UpdateIdentityOutcome, Identity, NewIdentity, RemoveGrantOutcome, RetireOutcome, MAX_IDENTITY_LABEL_CHARS};
use crate::proxy_auth::Caller;
use crate::registry::{canonical_pubkey, DepartmentRole};
use crate::state::AppState;
use axum::{
    body::Bytes,
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
    /// The members granted it, hex, leads included: for administrators and the leads of its
    /// department. Administrators may use every identity without a grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<String>>,
    /// The leads of its department (#147), hex: listed with `members`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leads: Option<Vec<String>>,
    /// Whether grants decide who may use the primary (#103, #147), as the bunker holds it: listed
    /// with `members`. False only before any identity is added beside the primary or any lead is
    /// appointed, while everyone who may sign may use the primary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grants_matter: Option<bool>,
    /// Belongs to no department (#147): only administrators use it.
    #[serde(default)]
    pub admin_only: bool,
    /// The caller's own role in its department (#147), when they hold a grant of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_role: Option<DepartmentRole>,
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

/// What `PATCH /api/bunker/identities/:pubkey` changes: its label, its administrator only mark
/// (#147), or both. At least one.
#[derive(Deserialize, Default)]
pub struct UpdateIdentityRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub admin_only: Option<bool>,
}

/// The role a grant gives (#147): `member` when omitted. `role` is Some(None) when it is present
/// but null, which is refused rather than read as omitted.
#[derive(Deserialize, Default)]
pub struct GrantRequest {
    #[serde(default, deserialize_with = "present")]
    pub role: Option<Option<String>>,
}

/// Marks a field that is present, even as null, so it is told apart from one that is omitted.
fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

/// An identity's department, for the routes that list it: its members and leads.
struct Department {
    members: Vec<String>,
    leads: Vec<String>,
    grants_matter: bool,
}

async fn response(state: &AppState, identity: Identity, department: Option<Department>, usage: Option<IdentityUsage>, my_role: Option<DepartmentRole>) -> IdentityResponse {
    let available = identity.retired_at.is_none() && state.keyring.is_identity(&identity.pubkey).await;
    let (members, leads, grants_matter) = match department {
        Some(d) => (Some(d.members), Some(d.leads), Some(d.grants_matter)),
        None => (None, None, None),
    };
    IdentityResponse {
        npub: crate::registry::npub(&identity.pubkey),
        pubkey: identity.pubkey,
        label: identity.label,
        is_primary: identity.is_primary,
        available,
        members,
        leads,
        grants_matter,
        admin_only: identity.admin_only,
        my_role,
        usage,
        retired_at: identity.retired_at,
    }
}

fn department(state: &AppState, reference: Option<&str>) -> Result<Department, ApiError> {
    // One snapshot, so the members and the access mode describe the same state.
    let (grants, grants_matter) = state.db.department_of(reference).map_err(database_error)?;
    Ok(Department {
        leads: grants.iter().filter(|g| g.role == Some(DepartmentRole::Lead)).map(|g| g.member_pubkey.clone()).collect(),
        members: grants.into_iter().map(|g| g.member_pubkey).collect(),
        grants_matter,
    })
}

fn usage_of(state: &AppState, reference: Option<&str>) -> Result<Option<IdentityUsage>, ApiError> {
    match reference {
        Some(hex) => {
            let (connections, tokens) = state.db.identity_usage(hex).map_err(database_error)?;
            Ok(Some(IdentityUsage { connections, tokens }))
        }
        None => Ok(None),
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

fn forbidden(message: &str) -> ApiError {
    api_error(StatusCode::FORBIDDEN, "forbidden", message)
}

/// Who may create an identity (#146, #147): administrators, and the lead of any department, whose
/// new identity starts a department they lead.
fn may_create(state: &AppState, pubkey: &str, administrator: bool) -> Result<(), ApiError> {
    if administrator || state.db.leads_any_department(pubkey).map_err(database_error)? {
        Ok(())
    } else {
        Err(forbidden("Only an administrator or a department lead can create identities."))
    }
}

/// Refuses a caller who is neither an administrator nor a lead of any department (#147), before
/// the identity a path names is looked up, so they learn nothing about it.
fn may_lead_somewhere(state: &AppState, pubkey: &str, administrator: bool, message: &str) -> Result<(), ApiError> {
    if administrator || state.db.leads_any_department(pubkey).map_err(database_error)? {
        Ok(())
    } else {
        Err(forbidden(message))
    }
}

/// Who manages an identity (#147): administrators, and the leads of its department. Never anyone
/// but an administrator on an administrator only identity, which has no leads.
fn may_manage(state: &AppState, pubkey: &str, administrator: bool, identity: &Identity, message: &str) -> Result<(), ApiError> {
    if administrator || state.db.may_lead(pubkey, identity.reference()).map_err(database_error)? {
        Ok(())
    } else {
        Err(forbidden(message))
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
                listed.push(response(&state, identity, None, None, None).await);
            }
            continue;
        }
        let reference = identity.reference().map(str::to_string);
        let my_role = if pubkey.is_empty() { None } else { state.db.find_grant(reference.as_deref(), &pubkey).map_err(database_error)?.and_then(|g| g.role) };
        if administrator || state.db.may_lead(&pubkey, reference.as_deref()).map_err(database_error)? {
            let department = department(&state, reference.as_deref())?;
            let usage = usage_of(&state, reference.as_deref())?;
            listed.push(response(&state, identity, Some(department), usage, my_role).await);
        } else if state.db.may_use_identity(&pubkey, reference.as_deref()).map_err(database_error)? {
            listed.push(response(&state, identity, None, None, my_role).await);
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
    may_create(&state, &by, administrator)?;
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
    // A lead's new identity starts a department they lead (#147), in the same transaction.
    let new = NewIdentity { admin_only: false, lead: if administrator { None } else { crate::audit::actor(&by) } };
    match state.db.add_identity_with(&pubkey, label, &key_file, crate::audit::actor(&by), new, Utc::now()).map_err(database_error)? {
        Ok(()) => {}
        // No longer a lead by the time it would be stored (#147): read in the same transaction.
        Err(AddIdentityRefusal::NotALead) => return Err(forbidden("Only an administrator or a department lead can create identities.")),
        Err(_) => return Err(api_error(StatusCode::CONFLICT, "key_in_use", "The generated key is already in use. Try again.")),
    }
    state.keyring.add(crate::signer::Signer::new(secret_key)).await;
    if let Err(e) = state.identities_changed().await {
        warn!("Created identity {}, but the NIP-46 subscription could not be updated yet: {}", pubkey, e);
    }
    info!("Created identity {}", pubkey);
    let identity = state.db.find_identity(&pubkey).map_err(database_error)?.ok_or_else(not_found)?;
    let usage = Some(IdentityUsage { connections: 0, tokens: 0 });
    let department = department(&state, identity.reference())?;
    let my_role = if administrator { None } else { Some(DepartmentRole::Lead) };
    Ok((StatusCode::CREATED, Json(response(&state, identity, Some(department), usage, my_role).await)))
}

/// Retire an identity (#146): revoke every connection and unused token that signs as it, remove
/// its grants, destroy its encrypted key, and stop answering requests to it. Its signing log and
/// audit events stay. The primary is never retired. DELETE /api/bunker/identities/:pubkey
pub async fn retire_identity(State(state): State<AppState>, who: Option<Extension<Caller>>, Path(key): Path<String>) -> Result<Json<Value>, ApiError> {
    let (by, administrator) = caller(who);
    const RETIRE: &str = "Only an administrator or a lead of its department can retire an identity.";
    may_lead_somewhere(&state, &by, administrator, RETIRE)?;
    let hex = canonical_pubkey(&key).map_err(|_| not_found())?;
    if !administrator {
        let identity = find(&state, &key)?;
        may_manage(&state, &by, administrator, &identity, RETIRE)?;
    }
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

/// Rename an identity, or mark or unmark it administrator only (#147).
/// PATCH /api/bunker/identities/:pubkey
///
/// A lead of its department may rename it; only an administrator changes the mark. Marking is
/// refused (409 `has_members`) while anyone but an administrator may use it. Everything is checked
/// before anything changes.
pub async fn rename_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(key): Path<String>,
    Json(request): Json<UpdateIdentityRequest>,
) -> Result<Json<Value>, ApiError> {
    let (by, administrator) = caller(who);
    const EDIT: &str = "Only an administrator or a lead of its department can rename an identity.";
    may_lead_somewhere(&state, &by, administrator, EDIT)?;
    let identity = find(&state, &key)?;
    may_manage(&state, &by, administrator, &identity, EDIT)?;
    let label = match request.label.as_deref().map(str::trim) {
        Some(label) if label.is_empty() || label.chars().count() > MAX_IDENTITY_LABEL_CHARS => {
            return Err(api_error(StatusCode::BAD_REQUEST, "invalid_label", "Give the identity a label of up to 100 characters."));
        }
        other => other,
    };
    if label.is_none() && request.admin_only.is_none() {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_label", "Give the identity a label of up to 100 characters."));
    }
    if request.admin_only.is_some() && !administrator {
        return Err(forbidden("Only an administrator can mark an identity administrator only."));
    }
    // The mark and the label change together or not at all, with their audit events.
    // A lead's authority is read again in that transaction (#147).
    let acting_lead = if administrator { None } else { Some(by.as_str()) };
    match state.db.update_identity(&identity.pubkey, identity.reference(), label, request.admin_only, crate::audit::actor(&by), acting_lead, Utc::now()).map_err(database_error)? {
        UpdateIdentityOutcome::Updated => {}
        UpdateIdentityOutcome::NotALead => return Err(forbidden(EDIT)),
        UpdateIdentityOutcome::NotFound => return Err(not_found()),
        UpdateIdentityOutcome::HasMembers => {
            return Err(api_error(
                StatusCode::CONFLICT,
                "has_members",
                "Members may still sign as this identity. Remove them from it first, so nobody loses access without it being recorded.",
            ))
        }
    }
    if let Some(admin_only) = request.admin_only {
        info!("Identity {} is {}", identity.pubkey, if admin_only { "administrator only" } else { "open to departments" });
    }
    if label.is_some() {
        info!("Renamed identity {}", identity.pubkey);
    }
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

/// The role a grant body asks for (#147): `member` for an empty body, which the console and older
/// clients send. A body that is not a valid `GrantRequest`, or names another role, is refused
/// before anything is written, never read as `member`.
fn grant_role(body: &[u8]) -> Result<DepartmentRole, ApiError> {
    const INVALID: &str = "A department role is member or lead.";
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(DepartmentRole::Member);
    }
    let invalid = || api_error(StatusCode::BAD_REQUEST, "invalid_role", INVALID);
    // An object only, since serde would also read a struct from an array. Read straight from the
    // bytes, so a repeated `role` is refused as a duplicate field rather than the last one kept.
    if body.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(invalid());
    }
    let request: GrantRequest = serde_json::from_slice(body).map_err(|_| invalid())?;
    match request.role {
        None => Ok(DepartmentRole::Member),
        Some(None) => Err(invalid()),
        Some(Some(role)) => role.parse().map_err(|_| invalid()),
    }
}

/// Grant a member an identity, adding them to its department with a `role` of `member` (the
/// default) or `lead` (#147). PUT /api/bunker/identities/:pubkey/members/:member
///
/// A lead of its department grants it as a `member`, and may make themselves a `member`; only an
/// administrator appoints a lead or makes another lead a member. An administrator only identity
/// refuses every grant (409 `admin_only`).
pub async fn grant_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path((key, member)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (by, administrator) = caller(who);
    const GRANT: &str = "Only an administrator or a lead of this department can add members to it.";
    may_lead_somewhere(&state, &by, administrator, GRANT)?;
    let identity = find(&state, &key)?;
    may_manage(&state, &by, administrator, &identity, GRANT)?;
    let role = grant_role(&body)?;
    let member = signing_member(&state, &member)?;
    if !administrator && role == DepartmentRole::Lead {
        return Err(forbidden("Only an administrator can appoint a lead."));
    }
    // A lead's authority, and whether the member is another lead, are read in the same transaction
    // as the write (#147).
    let acting_lead = if administrator { None } else { Some(by.as_str()) };
    match state.db.grant_identity_by(identity.reference(), &member, role, crate::audit::actor(&by), acting_lead, Utc::now()).map_err(database_error)? {
        GrantOutcome::Granted => info!("Granted {} the identity {} as {}", member, identity.pubkey, role),
        GrantOutcome::RoleChanged => info!("{} is now a {} of the identity {}", member, role, identity.pubkey),
        GrantOutcome::Unchanged => {}
        GrantOutcome::AdminOnly => {
            return Err(api_error(StatusCode::CONFLICT, "admin_only", "This identity is administrator only: it has no members, and only administrators use it."))
        }
        GrantOutcome::NotALead => return Err(forbidden(GRANT)),
        GrantOutcome::AnotherLead => return Err(forbidden("Only an administrator can remove another lead.")),
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
    let (by, administrator) = caller(who);
    const REMOVE: &str = "Only an administrator or a lead of this department can remove members from it.";
    may_lead_somewhere(&state, &by, administrator, REMOVE)?;
    let identity = find(&state, &key)?;
    may_manage(&state, &by, administrator, &identity, REMOVE)?;
    let member = canonical_pubkey(&member).map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid_pubkey", "That is not a valid npub or hex key."))?;
    // A lead removes a member, or themselves; another lead only an administrator (#147). Read in
    // the same transaction as the removal.
    let acting_lead = if administrator { None } else { Some(by.as_str()) };
    match state.db.remove_identity_grant_by(identity.reference(), &member, crate::audit::actor(&by), acting_lead, Utc::now()).map_err(database_error)? {
        RemoveGrantOutcome::Removed { connections, tokens } => {
            info!("Removed {}'s grant of {}: {} connections and {} tokens revoked", member, identity.pubkey, connections, tokens);
            Ok(Json(json!({ "success": true, "connections_revoked": connections, "tokens_revoked": tokens })))
        }
        RemoveGrantOutcome::NotGranted => Err(api_error(StatusCode::NOT_FOUND, "not_found", "That member is not granted that identity.")),
        RemoveGrantOutcome::NotALead => Err(forbidden(REMOVE)),
        RemoveGrantOutcome::AnotherLead => Err(forbidden("Only an administrator can remove another lead.")),
    }
}


/// Sign an event the console built as an identity (#150): its profile (kind 0), follow list
/// (kind 3), relay list (kind 10002), or a Blossom upload authorisation (kind 24242) for its
/// profile image. `console_signing::check` refuses anything else, and a refusal signs and logs
/// nothing. The identity must be this bunker's, not retired and unlocked. Each signature is logged
/// with the administrator or lead as the member and marked as the console's.
/// POST /api/bunker/identities/:pubkey/sign
pub async fn sign_as_identity(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Path(key): Path<String>,
    Json(template): Json<EventTemplate>,
) -> Result<Json<nostr::Event>, ApiError> {
    let (by, administrator) = caller(who);
    // Administrators, and the leads of its department (#147).
    const SIGN: &str = "Only an administrator or a lead of its department can edit an identity's profile.";
    may_lead_somewhere(&state, &by, administrator, SIGN)?;
    let identity = find(&state, &key)?;
    may_manage(&state, &by, administrator, &identity, SIGN)?;
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
