// The proxy-identity signature (#25). The three vectors are also asserted by
// frontend/tests/server/auth/proxy-signature.spec.ts, so the implementations cannot drift apart.
use bunker::proxy_auth::{
    proxy_signature, verify_proxy_request, Nonces, ProxyAuthError, ProxyHeaders, Signed, Verified, MAX_SKEW_SECONDS,
    SERVICE_IDENTITY,
};

const SECRET: &[u8] = b"bancwr-test-secret";
const NOW: i64 = 1_800_000_000;
const PATH: &str = "/api/bunker/whoami?x=1";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const BODY: &[u8] = br#"{"label":"x","kinds":[1]}"#;

fn pubkey() -> String {
    "ab".repeat(32)
}

fn sign(timestamp: i64, method: &str, path: &str, identity: &str, body: &[u8]) -> String {
    proxy_signature(SECRET, &Signed { timestamp, nonce: NONCE, method, path, identity, body })
}

fn verify(identity: &str, timestamp: i64, signature: &str, method: &str, path: &str, body: &[u8]) -> Result<String, ProxyAuthError> {
    let timestamp = timestamp.to_string();
    let headers = ProxyHeaders { identity: Some(identity), timestamp: Some(&timestamp), nonce: Some(NONCE), signature: Some(signature) };
    verify_proxy_request(SECRET, &headers, method, path, body, NOW).map(|v| v.identity)
}

#[test]
fn matches_the_vectors_shared_with_the_frontend() {
    let user = Signed { timestamp: NOW, nonce: NONCE, method: "get", path: PATH, identity: &pubkey(), body: b"" };
    assert_eq!(proxy_signature(SECRET, &user), "4a168ca21125d31242716977c40277da3b80864560281de9fc678a0940ea5267");

    let lookup = format!("/api/bunker/team/by-pubkey/{}", "cd".repeat(32));
    let service = Signed {
        timestamp: NOW,
        nonce: "fedcba9876543210fedcba9876543210",
        method: "GET",
        path: &lookup,
        identity: SERVICE_IDENTITY,
        body: b"",
    };
    assert_eq!(proxy_signature(SECRET, &service), "9096f34ab4eedd93bc53d4adfdecff42a7aea3a458f64be56a9af325462248f5");

    let with_body = Signed {
        timestamp: NOW,
        nonce: "00112233445566778899aabbccddeeff",
        method: "POST",
        path: "/api/bunker/connections/tokens",
        identity: &pubkey(),
        body: BODY,
    };
    assert_eq!(proxy_signature(SECRET, &with_body), "9570f30f7540846ef780bebc5642826c2ece117d0fe050723d3968e60fba86fd");
}

#[test]
fn accepts_a_valid_signature_for_a_pubkey_or_the_service() {
    let sig = sign(NOW, "GET", PATH, &pubkey(), b"");
    assert_eq!(verify(&pubkey(), NOW, &sig, "GET", PATH, b""), Ok(pubkey()));

    let sig = sign(NOW, "GET", PATH, SERVICE_IDENTITY, b"");
    assert_eq!(verify(SERVICE_IDENTITY, NOW, &sig, "GET", PATH, b""), Ok(SERVICE_IDENTITY.to_string()));

    let sig = sign(NOW, "POST", PATH, &pubkey(), BODY);
    assert_eq!(verify(&pubkey(), NOW, &sig, "POST", PATH, BODY), Ok(pubkey()));
}

#[test]
fn returns_the_timestamp_and_nonce_it_checked() {
    let sig = sign(NOW, "GET", PATH, &pubkey(), b"");
    let pubkey = pubkey();
    let timestamp = NOW.to_string();
    let headers = ProxyHeaders { identity: Some(&pubkey), timestamp: Some(&timestamp), nonce: Some(NONCE), signature: Some(&sig) };
    assert_eq!(
        verify_proxy_request(SECRET, &headers, "GET", PATH, b"", NOW),
        Ok(Verified { identity: pubkey.clone(), timestamp: NOW, nonce: NONCE.to_string() })
    );
}

#[test]
fn rejects_missing_headers() {
    let sig = sign(NOW, "GET", PATH, &pubkey(), b"");
    let pubkey = pubkey();
    let full = || ProxyHeaders { identity: Some(&pubkey), timestamp: Some("1"), nonce: Some(NONCE), signature: Some(&sig) };
    for headers in [
        ProxyHeaders { identity: None, ..full() },
        ProxyHeaders { timestamp: None, ..full() },
        ProxyHeaders { nonce: None, ..full() },
        ProxyHeaders { signature: None, ..full() },
    ] {
        assert_eq!(verify_proxy_request(SECRET, &headers, "GET", PATH, b"", NOW), Err(ProxyAuthError::Missing));
    }
}

#[test]
fn rejects_a_different_secret_method_path_identity_or_body() {
    let pubkey = pubkey();
    let other_secret =
        proxy_signature(b"not-the-secret", &Signed { timestamp: NOW, nonce: NONCE, method: "GET", path: PATH, identity: &pubkey, body: b"" });
    assert_eq!(verify(&pubkey, NOW, &other_secret, "GET", PATH, b""), Err(ProxyAuthError::BadSignature));

    let sig = sign(NOW, "GET", PATH, &pubkey, b"");
    assert_eq!(verify(&pubkey, NOW, &sig, "POST", PATH, b""), Err(ProxyAuthError::BadSignature));
    assert_eq!(verify(&pubkey, NOW, &sig, "GET", "/api/bunker/team", b""), Err(ProxyAuthError::BadSignature));
    assert_eq!(verify(&"cd".repeat(32), NOW, &sig, "GET", PATH, b""), Err(ProxyAuthError::BadSignature));
    // A user's signature does not become a service one.
    assert_eq!(verify(SERVICE_IDENTITY, NOW, &sig, "GET", PATH, b""), Err(ProxyAuthError::BadSignature));

    // The body is signed: another body, or a body added to a request signed without one, fails.
    let sig = sign(NOW, "POST", PATH, &pubkey, BODY);
    assert_eq!(verify(&pubkey, NOW, &sig, "POST", PATH, br#"{"label":"x","kinds":[0,1,3]}"#), Err(ProxyAuthError::BadSignature));
    let sig = sign(NOW, "POST", PATH, &pubkey, b"");
    assert_eq!(verify(&pubkey, NOW, &sig, "POST", PATH, BODY), Err(ProxyAuthError::BadSignature));
}

#[test]
fn rejects_a_signature_under_another_nonce() {
    let sig = sign(NOW, "GET", PATH, &pubkey(), b"");
    let pubkey = pubkey();
    let timestamp = NOW.to_string();
    let headers = ProxyHeaders {
        identity: Some(&pubkey),
        timestamp: Some(&timestamp),
        nonce: Some("ffffffffffffffffffffffffffffffff"),
        signature: Some(&sig),
    };
    assert_eq!(verify_proxy_request(SECRET, &headers, "GET", PATH, b"", NOW), Err(ProxyAuthError::BadSignature));
}

#[test]
fn rejects_a_stale_or_future_timestamp() {
    for timestamp in [NOW - MAX_SKEW_SECONDS - 1, NOW + MAX_SKEW_SECONDS + 1] {
        let sig = sign(timestamp, "GET", PATH, &pubkey(), b"");
        assert_eq!(verify(&pubkey(), timestamp, &sig, "GET", PATH, b""), Err(ProxyAuthError::Stale));
    }
    let edge = NOW - MAX_SKEW_SECONDS;
    let sig = sign(edge, "GET", PATH, &pubkey(), b"");
    assert!(verify(&pubkey(), edge, &sig, "GET", PATH, b"").is_ok());
}

#[test]
fn rejects_a_malformed_identity_nonce_or_timestamp() {
    for identity in ["npub1abc", "AB", &"AB".repeat(32), "Service", ""] {
        let sig = sign(NOW, "GET", PATH, identity, b"");
        assert_eq!(verify(identity, NOW, &sig, "GET", PATH, b""), Err(ProxyAuthError::Malformed), "{:?}", identity);
    }

    let pubkey = pubkey();
    let timestamp = NOW.to_string();
    let upper = NONCE.to_uppercase();
    let long = format!("{}00", NONCE);
    for nonce in ["", "0123", &upper, &long, "0123456789abcdef0123456789abcdeg"] {
        let sig = proxy_signature(SECRET, &Signed { timestamp: NOW, nonce, method: "GET", path: PATH, identity: &pubkey, body: b"" });
        let headers = ProxyHeaders { identity: Some(&pubkey), timestamp: Some(&timestamp), nonce: Some(nonce), signature: Some(&sig) };
        assert_eq!(verify_proxy_request(SECRET, &headers, "GET", PATH, b"", NOW), Err(ProxyAuthError::Malformed), "{:?}", nonce);
    }

    let sig = sign(NOW, "GET", PATH, &pubkey, b"");
    let headers = ProxyHeaders { identity: Some(&pubkey), timestamp: Some("soon"), nonce: Some(NONCE), signature: Some(&sig) };
    assert_eq!(verify_proxy_request(SECRET, &headers, "GET", PATH, b"", NOW), Err(ProxyAuthError::Malformed));
}

/// A set for a process that started a minute before `NOW`.
fn nonces() -> Nonces {
    Nonces::started_at(NOW - 60)
}

#[test]
fn a_nonce_is_accepted_once() {
    let nonces = nonces();
    assert_eq!(nonces.accept(NONCE, NOW, NOW), Ok(()));
    assert_eq!(nonces.accept(NONCE, NOW, NOW), Err(ProxyAuthError::Replayed));
    assert_eq!(nonces.accept(NONCE, NOW, NOW + MAX_SKEW_SECONDS), Err(ProxyAuthError::Replayed));
    assert_eq!(nonces.accept("fedcba9876543210fedcba9876543210", NOW, NOW), Ok(()));
}

#[test]
fn a_nonce_is_forgotten_once_its_timestamp_leaves_the_window() {
    let nonces = nonces();
    assert_eq!(nonces.accept(NONCE, NOW, NOW), Ok(()));
    // By now the window refuses that timestamp as stale, so the nonce no longer needs keeping.
    let later = NOW + MAX_SKEW_SECONDS + 1;
    assert_eq!(nonces.accept(NONCE, later, later), Ok(()));
}

#[test]
fn a_nonce_is_still_refused_between_sweeps() {
    // Several requests in the same second sweep once; the set still holds every nonce.
    let nonces = nonces();
    for nonce in [NONCE, "fedcba9876543210fedcba9876543210", "00112233445566778899aabbccddeeff"] {
        assert_eq!(nonces.accept(nonce, NOW, NOW), Ok(()));
    }
    for nonce in [NONCE, "fedcba9876543210fedcba9876543210", "00112233445566778899aabbccddeeff"] {
        assert_eq!(nonces.accept(nonce, NOW, NOW), Err(ProxyAuthError::Replayed));
    }
}

#[test]
fn a_timestamp_from_before_the_process_started_is_refused() {
    // A request signed for the process before a restart, still inside the time window, is
    // refused by the new process even though its set of nonces starts empty.
    let started = NOW;
    let nonces = Nonces::started_at(started);
    let before = started - 1;
    assert!((NOW - before).abs() <= MAX_SKEW_SECONDS, "inside the window");
    assert_eq!(nonces.accept(NONCE, before, NOW), Err(ProxyAuthError::Stale));
    assert_eq!(nonces.accept(NONCE, started - MAX_SKEW_SECONDS, NOW), Err(ProxyAuthError::Stale));

    // Signed from the second the process started on: accepted, once.
    assert_eq!(nonces.accept(NONCE, started, NOW), Ok(()));
    assert_eq!(nonces.accept(NONCE, started, NOW + 1), Err(ProxyAuthError::Replayed));
    assert_eq!(nonces.accept("fedcba9876543210fedcba9876543210", started + 5, started + 5), Ok(()));
}

#[test]
fn a_set_made_now_accepts_a_request_signed_now() {
    let nonces = Nonces::default();
    let now = chrono::Utc::now().timestamp();
    assert_eq!(nonces.accept(NONCE, now, now), Ok(()));
}
