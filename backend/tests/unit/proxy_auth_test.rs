// The proxy-identity signature (#25). The two vectors are also asserted by
// frontend/tests/server/auth/proxy-signature.spec.ts, so the implementations cannot drift apart.
use bunker::proxy_auth::{proxy_signature, verify_proxy_request, ProxyAuthError, MAX_SKEW_SECONDS, SERVICE_IDENTITY};

const SECRET: &[u8] = b"bancwr-test-secret";
const NOW: i64 = 1_800_000_000;
const PATH: &str = "/api/bunker/whoami?x=1";

fn pubkey() -> String {
    "ab".repeat(32)
}

fn verify(identity: &str, timestamp: i64, signature: &str, method: &str, path: &str) -> Result<String, ProxyAuthError> {
    verify_proxy_request(SECRET, Some(identity), Some(&timestamp.to_string()), Some(signature), method, path, NOW)
}

#[test]
fn matches_the_vectors_shared_with_the_frontend() {
    assert_eq!(proxy_signature(SECRET, NOW, "get", PATH, &pubkey()), "72014519598768d6aa027face9fa3f2f1377a6e6f756ab53b862077311a3b4fc");
    let lookup = format!("/api/bunker/team/by-pubkey/{}", "cd".repeat(32));
    assert_eq!(proxy_signature(SECRET, NOW, "GET", &lookup, SERVICE_IDENTITY), "5eaf28079c0017b4f359cf886f5c045b9bcf7986fac3c8e92cd7cc2468ddb9af");
}

#[test]
fn accepts_a_valid_signature_for_a_pubkey_or_the_service() {
    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    assert_eq!(verify(&pubkey(), NOW, &sig, "GET", PATH), Ok(pubkey()));

    let sig = proxy_signature(SECRET, NOW, "GET", PATH, SERVICE_IDENTITY);
    assert_eq!(verify(SERVICE_IDENTITY, NOW, &sig, "GET", PATH), Ok(SERVICE_IDENTITY.to_string()));
}

#[test]
fn rejects_missing_headers() {
    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    assert_eq!(verify_proxy_request(SECRET, None, Some("1"), Some(&sig), "GET", PATH, NOW), Err(ProxyAuthError::Missing));
    assert_eq!(verify_proxy_request(SECRET, Some(&pubkey()), None, Some(&sig), "GET", PATH, NOW), Err(ProxyAuthError::Missing));
    assert_eq!(verify_proxy_request(SECRET, Some(&pubkey()), Some("1"), None, "GET", PATH, NOW), Err(ProxyAuthError::Missing));
}

#[test]
fn rejects_a_different_secret_method_path_or_identity() {
    let other_secret = proxy_signature(b"not-the-secret", NOW, "GET", PATH, &pubkey());
    assert_eq!(verify(&pubkey(), NOW, &other_secret, "GET", PATH), Err(ProxyAuthError::BadSignature));

    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    assert_eq!(verify(&pubkey(), NOW, &sig, "POST", PATH), Err(ProxyAuthError::BadSignature));
    assert_eq!(verify(&pubkey(), NOW, &sig, "GET", "/api/bunker/team"), Err(ProxyAuthError::BadSignature));
    assert_eq!(verify(&"cd".repeat(32), NOW, &sig, "GET", PATH), Err(ProxyAuthError::BadSignature));
    // A user's signature does not become a service one.
    assert_eq!(verify(SERVICE_IDENTITY, NOW, &sig, "GET", PATH), Err(ProxyAuthError::BadSignature));
}

#[test]
fn rejects_a_stale_or_future_timestamp() {
    for timestamp in [NOW - MAX_SKEW_SECONDS - 1, NOW + MAX_SKEW_SECONDS + 1] {
        let sig = proxy_signature(SECRET, timestamp, "GET", PATH, &pubkey());
        assert_eq!(verify(&pubkey(), timestamp, &sig, "GET", PATH), Err(ProxyAuthError::Stale));
    }
    let edge = NOW - MAX_SKEW_SECONDS;
    let sig = proxy_signature(SECRET, edge, "GET", PATH, &pubkey());
    assert!(verify(&pubkey(), edge, &sig, "GET", PATH).is_ok());
}

#[test]
fn rejects_a_malformed_identity_or_timestamp() {
    for identity in ["npub1abc", "AB", &"AB".repeat(32), "Service", ""] {
        let sig = proxy_signature(SECRET, NOW, "GET", PATH, identity);
        assert_eq!(verify(identity, NOW, &sig, "GET", PATH), Err(ProxyAuthError::Malformed), "{:?}", identity);
    }
    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    assert_eq!(
        verify_proxy_request(SECRET, Some(&pubkey()), Some("soon"), Some(&sig), "GET", PATH, NOW),
        Err(ProxyAuthError::Malformed)
    );
}
