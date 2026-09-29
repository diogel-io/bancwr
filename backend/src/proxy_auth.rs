//! SPIKE (diogel-io/bancwr#23). Checks the identity the frontend's Nitro proxy attaches to each
//! request. The proxy has already verified the user's session; it names the pubkey and signs
//! `v1\n<timestamp>\n<METHOD>\n<path?query>\n<pubkey>` with HMAC-SHA256 under a secret only it and
//! the bunker hold (BANCWR_PROXY_SECRET here, NUXT_PROXY_SECRET there). A caller that reaches the
//! bunker's port directly has no secret, so it cannot claim anyone's identity.
//!
//! The same test vector is asserted by frontend/tests/server/auth/proxy-signature.spec.ts.

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

pub const PUBKEY_HEADER: &str = "x-bancwr-pubkey";
pub const TIMESTAMP_HEADER: &str = "x-bancwr-timestamp";
pub const SIGNATURE_HEADER: &str = "x-bancwr-signature";

/// How far a signed request's timestamp may be from the bunker's clock, in seconds.
pub const MAX_SKEW_SECONDS: i64 = 30;

/// The caller's pubkey (hex), attached to the request once its proxy signature checks out.
#[derive(Clone, Debug, PartialEq)]
pub struct ProxyIdentity {
    pub pubkey: String,
}

#[derive(Debug, PartialEq)]
pub enum ProxyAuthError {
    Missing,
    Malformed,
    Stale,
    BadSignature,
}

/// Lowercase hex HMAC-SHA256 of the canonical string.
pub fn proxy_signature(secret: &[u8], timestamp: i64, method: &str, path: &str, pubkey: &str) -> String {
    let canonical = format!("v1\n{}\n{}\n{}\n{}", timestamp, method.to_uppercase(), path, pubkey);
    let mut engine = hmac::HmacEngine::<sha256::Hash>::new(secret);
    engine.input(canonical.as_bytes());
    let mac = hmac::Hmac::<sha256::Hash>::from_engine(engine);
    mac.to_byte_array().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Compares in time independent of where the inputs first differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn verify_proxy_request(
    secret: &[u8],
    pubkey: Option<&str>,
    timestamp: Option<&str>,
    signature: Option<&str>,
    method: &str,
    path: &str,
    now: i64,
) -> Result<ProxyIdentity, ProxyAuthError> {
    let (pubkey, timestamp, signature) = match (pubkey, timestamp, signature) {
        (Some(p), Some(t), Some(s)) => (p, t, s),
        _ => return Err(ProxyAuthError::Missing),
    };
    if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(ProxyAuthError::Malformed);
    }
    let timestamp: i64 = timestamp.parse().map_err(|_| ProxyAuthError::Malformed)?;
    if (now - timestamp).abs() > MAX_SKEW_SECONDS {
        return Err(ProxyAuthError::Stale);
    }
    let expected = proxy_signature(secret, timestamp, method, path, pubkey);
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(ProxyAuthError::BadSignature);
    }
    Ok(ProxyIdentity { pubkey: pubkey.to_string() })
}

/// Middleware: 401 unless the proxy signature is valid; otherwise the identity is available to
/// handlers as `Extension<ProxyIdentity>`. #25 adds the registry and role checks after this.
pub async fn require_proxy_identity(
    State(secret): State<Arc<[u8]>>,
    mut request: Request,
    next: Next,
) -> Response {
    // Read into owned values inside a block: a borrow of the request held across the `.await`
    // below would make this future !Send (Body is not Sync).
    let result = {
        let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let path = request.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
        verify_proxy_request(
            &secret,
            header(PUBKEY_HEADER).as_deref(),
            header(TIMESTAMP_HEADER).as_deref(),
            header(SIGNATURE_HEADER).as_deref(),
            request.method().as_str(),
            &path,
            chrono::Utc::now().timestamp(),
        )
    };
    match result {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(reason) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "not_authenticated", "reason": format!("{:?}", reason) })),
        )
            .into_response(),
    }
}
