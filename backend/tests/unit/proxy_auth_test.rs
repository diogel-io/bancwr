// SPIKE (#23). The proxy-signature check the frontend's Nitro proxy is trusted through.
use bunker::proxy_auth::{proxy_signature, verify_proxy_request, ProxyAuthError, MAX_SKEW_SECONDS};

const SECRET: &[u8] = b"bancwr-test-secret";
const NOW: i64 = 1_800_000_000;
const PATH: &str = "/api/bunker/whoami?x=1";

fn pubkey() -> String {
    "ab".repeat(32)
}

#[test]
fn matches_the_vector_shared_with_the_frontend() {
    // frontend/tests/server/auth/proxy-signature.spec.ts asserts the same value.
    assert_eq!(
        proxy_signature(SECRET, NOW, "get", PATH, &pubkey()),
        "72014519598768d6aa027face9fa3f2f1377a6e6f756ab53b862077311a3b4fc"
    );
}

#[test]
fn accepts_a_valid_signature() {
    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    let identity = verify_proxy_request(SECRET, Some(&pubkey()), Some(&NOW.to_string()), Some(&sig), "GET", PATH, NOW)
        .expect("valid");
    assert_eq!(identity.pubkey, pubkey());
}

#[test]
fn rejects_missing_headers() {
    let result = verify_proxy_request(SECRET, None, None, None, "GET", PATH, NOW);
    assert_eq!(result, Err(ProxyAuthError::Missing));
}

#[test]
fn rejects_a_different_secret_path_method_or_pubkey() {
    let sig = proxy_signature(b"not-the-secret", NOW, "GET", PATH, &pubkey());
    let ts = NOW.to_string();
    assert_eq!(verify_proxy_request(SECRET, Some(&pubkey()), Some(&ts), Some(&sig), "GET", PATH, NOW), Err(ProxyAuthError::BadSignature));

    let sig = proxy_signature(SECRET, NOW, "GET", PATH, &pubkey());
    assert_eq!(verify_proxy_request(SECRET, Some(&pubkey()), Some(&ts), Some(&sig), "POST", PATH, NOW), Err(ProxyAuthError::BadSignature));
    assert_eq!(verify_proxy_request(SECRET, Some(&pubkey()), Some(&ts), Some(&sig), "GET", "/api/bunker/team", NOW), Err(ProxyAuthError::BadSignature));
    let other = "cd".repeat(32);
    assert_eq!(verify_proxy_request(SECRET, Some(&other), Some(&ts), Some(&sig), "GET", PATH, NOW), Err(ProxyAuthError::BadSignature));
}

#[test]
fn rejects_a_stale_timestamp() {
    let old = NOW - MAX_SKEW_SECONDS - 1;
    let sig = proxy_signature(SECRET, old, "GET", PATH, &pubkey());
    assert_eq!(
        verify_proxy_request(SECRET, Some(&pubkey()), Some(&old.to_string()), Some(&sig), "GET", PATH, NOW),
        Err(ProxyAuthError::Stale)
    );
}

#[test]
fn rejects_a_malformed_pubkey() {
    let sig = proxy_signature(SECRET, NOW, "GET", PATH, "npub1abc");
    assert_eq!(
        verify_proxy_request(SECRET, Some("npub1abc"), Some(&NOW.to_string()), Some(&sig), "GET", PATH, NOW),
        Err(ProxyAuthError::Malformed)
    );
}
