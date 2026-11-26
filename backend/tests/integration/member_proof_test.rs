// SPIKE (#151): a bunker that checks each member's own sign-in. With member proof on, the console's
// HMAC alone no longer lets it claim a member: the member's NIP-98 event, carrying a challenge this
// bunker issued, starts a bunker session, and only that session's pubkey is accepted.
#[path = "../common/mod.rs"]
mod common;

use bunker::member_proof::SESSION_HEADER;
use bunker::registry::Role;
use nostr::base64::Engine as _;
use nostr::hashes::{sha256, Hash};
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const CONSOLE: &str = "http://localhost:3001";

fn sha256_hex(data: &[u8]) -> String {
    sha256::Hash::hash(data).to_byte_array().iter().map(|b| format!("{:02x}", b)).collect()
}

/// The `Authorization` header and body a member's sign-in sends, naming `url`.
fn sign_in(keys: &Keys, proofs: &[&str], url: &str) -> (String, String) {
    let body = json!({ "challenge": "the console's own", "proofs": proofs }).to_string();
    let event = EventBuilder::new(Kind::Custom(27235), "")
        .tags([
            Tag::parse(["u", url]).unwrap(),
            Tag::parse(["method", "POST"]).unwrap(),
            Tag::parse(["payload", &sha256_hex(body.as_bytes())]).unwrap(),
        ])
        .sign_with_keys(keys)
        .unwrap();
    let header = format!("Nostr {}", nostr::base64::engine::general_purpose::STANDARD.encode(event.as_json()));
    (header, body)
}

async fn challenge(app: &common::TestApp) -> String {
    let response = app.signed(Method::POST, "/api/bunker/auth/challenge", "service").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.json::<Value>().await.unwrap()["challenge"].as_str().unwrap().to_string()
}

async fn prove(app: &common::TestApp, authorization: &str, body: &str) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/auth/proof", "service")
        .json(&json!({ "authorization": authorization, "body": body }))
        .send()
        .await
        .unwrap()
}

async fn session(app: &common::TestApp, keys: &Keys) -> String {
    let c = challenge(app).await;
    let (authorization, body) = sign_in(keys, &[&c], &format!("{CONSOLE}/api/auth/login"));
    let response = prove(app, &authorization, &body).await;
    assert_eq!(response.status(), StatusCode::OK);
    response.json::<Value>().await.unwrap()["session"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_consoles_secret_alone_no_longer_claims_a_member() {
    let app = common::spawn_with_member_proof(CONSOLE, &[]).await;
    let admin = app.admin.public_key().to_hex();

    // The HMAC is valid, but the administrator never signed in to this bunker.
    let response = app.signed(Method::GET, "/api/bunker/team", &admin).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.json::<Value>().await.unwrap()["reason"], "member_proof_required");

    // After their own sign-in, with the session it started, they are let in.
    let token = session(&app, &app.admin).await;
    let response = app.signed(Method::GET, "/api/bunker/team", &admin).header(SESSION_HEADER, &token).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // That session is the administrator's: it does not let the console claim someone else.
    let signer = app.register(Role::Signer).public_key().to_hex();
    let response = app.signed(Method::GET, "/api/bunker/identities", &signer).header(SESSION_HEADER, &token).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // The service identity still needs no session: sign-in has to start somewhere.
    let response = app.signed(Method::GET, "/api/bunker/status", "service").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_sign_in_starts_a_session_only_once_and_only_as_the_rules_allow() {
    let sales = Keys::generate();
    let app = common::spawn_with_member_proof(CONSOLE, &[("Sales", sales.clone())]).await;
    let login = format!("{CONSOLE}/api/auth/login");

    // Used once: the same event and body again find the challenge gone.
    let c = challenge(&app).await;
    let (authorization, body) = sign_in(&app.admin, &[&c], &login);
    assert_eq!(prove(&app, &authorization, &body).await.status(), StatusCode::OK);
    let again = prove(&app, &authorization, &body).await;
    assert_eq!(again.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(again.json::<Value>().await.unwrap()["reason"], "unknown_challenge");

    // A challenge this bunker never issued, as another bunker's would be.
    let (authorization, body) = sign_in(&app.admin, &["0".repeat(64).as_str()], &login);
    assert_eq!(prove(&app, &authorization, &body).await.json::<Value>().await.unwrap()["reason"], "unknown_challenge");

    // A sign-in for another console.
    let c = challenge(&app).await;
    let (authorization, body) = sign_in(&app.admin, &[&c], "https://elsewhere.example/api/auth/login");
    assert_eq!(prove(&app, &authorization, &body).await.json::<Value>().await.unwrap()["reason"], "wrong_url");

    // A body that is not the one signed.
    let c = challenge(&app).await;
    let (authorization, _) = sign_in(&app.admin, &[&c], &login);
    let other = json!({ "challenge": "x", "proofs": [c] }).to_string();
    assert_eq!(prove(&app, &authorization, &other).await.json::<Value>().await.unwrap()["reason"], "payload_mismatch");

    // Rule 10, on this bunker: none of its identities signs in.
    let c = challenge(&app).await;
    let (authorization, body) = sign_in(&sales, &[&c], &login);
    let refused = prove(&app, &authorization, &body).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<Value>().await.unwrap()["reason"], "bunker_key");

    // A key this bunker's team does not hold.
    let c = challenge(&app).await;
    let (authorization, body) = sign_in(&Keys::generate(), &[&c], &login);
    let refused = prove(&app, &authorization, &body).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "not_registered");
}

#[tokio::test]
async fn one_bunkers_secret_is_refused_by_another() {
    let app = common::spawn_with_member_proof(CONSOLE, &[]).await;
    let other_secret = "another-bunkers-proxy-secret-0123456789abcdef";
    let response = app
        .signed_with(Method::POST, "/api/bunker/auth/challenge", "service", other_secret, chrono::Utc::now().timestamp())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.json::<Value>().await.unwrap()["reason"], "bad_signature");
}

#[tokio::test]
async fn without_member_proof_the_routes_are_off_and_the_hmac_decides_as_before() {
    let app = common::spawn(true).await;
    let response = app.signed(Method::POST, "/api/bunker/auth/challenge", "service").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app.get("/api/bunker/team").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
