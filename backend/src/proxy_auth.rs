//! Who is calling `/api/bunker/*` (#25).
//!
//! The browser never calls the bunker. It calls the frontend's Nitro server, which proxies to the
//! bunker and attaches the caller's identity, signed with HMAC-SHA256 under a secret only the two
//! hold (BANCWR_PROXY_SECRET here, NUXT_PROXY_SECRET there). The bunker trusts no identity without
//! a valid signature, so a caller reaching its port directly cannot claim to be anyone. See the
//! sign-in decision record (diogel-io/workspace 10-products/bancwr/architecture/
//! nostr-session-auth-adr.md), "Identity from the proxy to the bunker".
//!
//! An identity is a signed-in user's lowercase hex pubkey, or `service`: Nitro acting for itself
//! before anyone is signed in, which may only read the bunker's status and look a key up.
//!
//! frontend/tests/server/auth/proxy-signature.spec.ts asserts the same test vector.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use nostr::hashes::{hmac, sha256, Hash, HashEngine};
use serde_json::json;

use crate::db::Database;
use crate::registry::{npub, Role};

pub const IDENTITY_HEADER: &str = "x-bancwr-identity";
pub const TIMESTAMP_HEADER: &str = "x-bancwr-timestamp";
pub const SIGNATURE_HEADER: &str = "x-bancwr-signature";

/// The identity Nitro uses for its own calls.
pub const SERVICE_IDENTITY: &str = "service";

/// How far a signed request's timestamp may be from the bunker's clock, in seconds.
pub const MAX_SKEW_SECONDS: i64 = 30;

#[derive(Debug, PartialEq, Eq)]
pub enum ProxyAuthError {
    Missing,
    Malformed,
    Stale,
    BadSignature,
}

impl ProxyAuthError {
    fn reason(&self) -> &'static str {
        match self {
            ProxyAuthError::Missing => "missing",
            ProxyAuthError::Malformed => "malformed",
            ProxyAuthError::Stale => "stale",
            ProxyAuthError::BadSignature => "bad_signature",
        }
    }
}

/// Lowercase hex HMAC-SHA256 of `v1\n<timestamp>\n<METHOD>\n<path?query>\n<identity>`.
pub fn proxy_signature(secret: &[u8], timestamp: i64, method: &str, path: &str, identity: &str) -> String {
    let canonical = format!("v1\n{}\n{}\n{}\n{}", timestamp, method.to_uppercase(), path, identity);
    let mut engine = hmac::HmacEngine::<sha256::Hash>::new(secret);
    engine.input(canonical.as_bytes());
    let mac = hmac::Hmac::<sha256::Hash>::from_engine(engine);
    mac.to_byte_array().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Compares in time independent of where the inputs first differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Checks the signature and returns the identity it vouches for.
pub fn verify_proxy_request(
    secret: &[u8],
    identity: Option<&str>,
    timestamp: Option<&str>,
    signature: Option<&str>,
    method: &str,
    path: &str,
    now: i64,
) -> Result<String, ProxyAuthError> {
    let (identity, timestamp, signature) = match (identity, timestamp, signature) {
        (Some(i), Some(t), Some(s)) => (i, t, s),
        _ => return Err(ProxyAuthError::Missing),
    };
    if identity != SERVICE_IDENTITY && !is_hex_pubkey(identity) {
        return Err(ProxyAuthError::Malformed);
    }
    let timestamp: i64 = timestamp.parse().map_err(|_| ProxyAuthError::Malformed)?;
    if (now - timestamp).abs() > MAX_SKEW_SECONDS {
        return Err(ProxyAuthError::Stale);
    }
    let expected = proxy_signature(secret, timestamp, method, path, identity);
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(ProxyAuthError::BadSignature);
    }
    Ok(identity.to_string())
}

/// Who may call a group of routes: the backend's one access matrix (#25, #77). Administrator is
/// a superset of the other roles. `frontend/app/utils/access.ts` is the console's matching page
/// matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Bunker health: every registered role, and the service identity.
    Health,
    /// Administrators only: logs, metrics, config, bunker relays, adding and removing members,
    /// connection tokens.
    Administrator,
    /// Administrators and viewers: reading the team (#77).
    TeamReader,
    /// Administrators, viewers, and the service identity: the key lookup, which sign-in needs
    /// before any session exists, and a viewer's member profile page (#77).
    TeamReaderOrService,
    /// Administrators and signers, not the service identity: routes that scope what they return
    /// to the caller (#53's connection list, #77's own recent signatures).
    Signer,
}

impl Access {
    fn allows_role(self, role: Role) -> bool {
        match self {
            Access::Health => true,
            Access::Administrator => role == Role::Administrator,
            Access::TeamReader | Access::TeamReaderOrService => matches!(role, Role::Administrator | Role::Viewer),
            Access::Signer => role.can_sign(),
        }
    }

    fn allows_service(self) -> bool {
        matches!(self, Access::Health | Access::TeamReaderOrService)
    }
}

/// The verified caller, available to handlers as `Extension<Caller>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    Service,
    Member { pubkey: String, role: Role },
}

#[derive(Clone)]
pub struct Guard {
    pub secret: Arc<[u8]>,
    pub db: Arc<Database>,
    pub access: Access,
}

fn refuse(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}

/// Middleware for a group of `/api/bunker/*` routes:
/// 401 `not_authenticated` without a valid signature; 403 `not_registered` for a key not in the
/// vault; 403 `forbidden` for a role (or the service identity) the group does not allow.
/// The role is read from the vault on every request, so a removed member loses access at once.
pub async fn guard(State(guard): State<Guard>, mut request: Request, next: Next) -> Response {
    // Owned values inside a block: a borrow of the request held across the `.await` below would
    // make this future !Send (Body is not Sync).
    let verified = {
        let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let path = request.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
        verify_proxy_request(
            &guard.secret,
            header(IDENTITY_HEADER).as_deref(),
            header(TIMESTAMP_HEADER).as_deref(),
            header(SIGNATURE_HEADER).as_deref(),
            request.method().as_str(),
            &path,
            chrono::Utc::now().timestamp(),
        )
    };
    let identity = match verified {
        Ok(identity) => identity,
        Err(e) => return refuse(StatusCode::UNAUTHORIZED, json!({ "error": "not_authenticated", "reason": e.reason() })),
    };

    let caller = if identity == SERVICE_IDENTITY {
        if !guard.access.allows_service() {
            return refuse(StatusCode::FORBIDDEN, json!({ "error": "forbidden" }));
        }
        Caller::Service
    } else {
        let member = match guard.db.find_member_by_pubkey(&identity) {
            Ok(member) => member,
            Err(e) => {
                tracing::error!("Failed to look up caller: {}", e);
                return refuse(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "database_error" }));
            }
        };
        let Some(member) = member else {
            return refuse(StatusCode::FORBIDDEN, json!({ "error": "not_registered", "npub": npub(&identity) }));
        };
        // A stored role outside the three (only possible before #24) grants nothing.
        match member.role() {
            Some(role) if guard.access.allows_role(role) => Caller::Member { pubkey: identity, role },
            _ => return refuse(StatusCode::FORBIDDEN, json!({ "error": "forbidden" })),
        }
    };

    request.extensions_mut().insert(caller);
    next.run(request).await
}
