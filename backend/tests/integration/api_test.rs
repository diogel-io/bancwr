// The bunker API through the proxy identity, as the frontend will call it (#25).
#[path = "../common/mod.rs"]
mod common;

use chrono::Utc;
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::Value;

#[tokio::test]
async fn test_api_health_check_needs_no_credentials() {
    let app = common::spawn(true).await;

    let res = app.unsigned(Method::GET, "/health").send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    // Exactly this and nothing more: /health stays a cheap container probe. The version is on
    // /api/bunker/status instead (#35).
    assert_eq!(body, serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn test_api_status() {
    let app = common::spawn(true).await;

    let res = app.get("/api/bunker/status").send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["status"], "healthy");
    assert_eq!(body["pubkey"], app.bunker.public_key().to_bech32().unwrap());
    assert_eq!(body["version"], "0.0.0");
    // Each check by name, as the frontend reads them (#27). NIP-46 is off in the test app.
    assert_eq!(
        body["checks"],
        serde_json::json!([
            { "name": "signer", "status": "pass", "detail": "The signing key signs and verifies." },
            { "name": "database", "status": "pass", "detail": "The database answers." },
            { "name": "relays", "status": "disabled", "detail": "NIP-46 is turned off (NIP46_ENABLED), so no relays are used." },
            { "name": "administrator", "status": "pass", "detail": "An administrator is registered." }
        ])
    );
}

#[tokio::test]
async fn test_api_get_logs() {
    let app = common::spawn(true).await;
    // POST /sign is gone (#25), so the log is arranged directly.
    let pubkey = app.bunker.public_key().to_bech32().unwrap();
    app.db.log_signing_event("event-for-logs-test", &pubkey, None, None, 1, Utc::now()).unwrap();

    let res = app.get("/api/bunker/logs").send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["event_id"], "event-for-logs-test");
    assert_eq!(logs[0]["event_kind"], 1);
    assert_eq!(logs[0]["pubkey"], pubkey);
    // Logged without a connection: no member (diogel-io/workspace#38).
    assert!(logs[0]["member_pubkey"].is_null() && logs[0]["member_name"].is_null() && logs[0]["connection_id"].is_null());
}

#[tokio::test]
async fn test_api_metrics() {
    let app = common::spawn(true).await;

    let before: Value = app.get("/api/bunker/metrics").send().await.unwrap().json().await.unwrap();
    app.get("/api/bunker/status").send().await.unwrap();
    let after: Value = app.get("/api/bunker/metrics").send().await.unwrap().json().await.unwrap();

    // The status call is counted.
    assert!(after["http_requests"].as_u64().unwrap() > before["http_requests"].as_u64().unwrap());
    assert_eq!(after["total_signatures"], 0);
}

#[tokio::test]
async fn test_api_sign_is_gone() {
    // POST /sign signed anything with the bunker's key for anyone who could reach it (#25).
    let app = common::spawn(true).await;
    let res = app.request(Method::POST, "/sign").json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_api_is_open_without_a_proxy_secret() {
    // Until sign-in (#11) requires BANCWR_PROXY_SECRET, an unset secret leaves the API as it was.
    let app = common::spawn(false).await;
    let res = app.unsigned(Method::GET, "/api/bunker/team").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_api_my_logs_are_the_callers_own() {
    // GET /api/bunker/logs/mine (#77): only signatures made for the caller as the member
    // (diogel-io/workspace#38), for the signer dashboard. Never another member's, never one only
    // the client key matches.
    let app = common::spawn(true).await;
    let alice = app.register(bunker::registry::Role::Signer).public_key().to_hex();
    let bob = app.register(bunker::registry::Role::Signer).public_key().to_hex();
    let now = Utc::now();
    app.db.log_signing_event("alice-1", &Keys::generate().public_key().to_hex(), Some(&alice), Some("c-a"), 1, now).unwrap();
    app.db.log_signing_event("bob-1", &Keys::generate().public_key().to_hex(), Some(&bob), Some("c-b"), 7, now).unwrap();
    app.db.log_signing_event("alice-2", &Keys::generate().public_key().to_hex(), Some(&alice), Some("c-a"), 1, now + chrono::Duration::seconds(1)).unwrap();
    app.db.log_signing_event("unattributed", &alice, None, None, 1, now).unwrap();

    let mine = |who: String| {
        let request = app.signed(Method::GET, "/api/bunker/logs/mine", &who);
        async move {
            let res = request.send().await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            res.json::<Vec<Value>>().await.unwrap().iter().map(|l| l["event_id"].as_str().unwrap().to_string()).collect::<Vec<_>>()
        }
    };
    assert_eq!(mine(alice.clone()).await, vec!["alice-2", "alice-1"]);
    assert_eq!(mine(bob.clone()).await, vec!["bob-1"]);
    assert!(mine(app.admin.public_key().to_hex()).await.is_empty(), "an administrator's own, not everyone's");

    let entry: Vec<Value> = app.signed(Method::GET, "/api/bunker/logs/mine", &bob).send().await.unwrap().json().await.unwrap();
    assert_eq!(entry[0]["member_pubkey"], bob);
    assert_eq!(entry[0]["event_kind"], 7);
}
