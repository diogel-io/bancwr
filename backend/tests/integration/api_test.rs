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
}

#[tokio::test]
async fn test_api_get_logs() {
    let app = common::spawn(true).await;
    // POST /sign is gone (#25), so the log is arranged directly.
    let pubkey = app.bunker.public_key().to_bech32().unwrap();
    app.db.log_signing_event("event-for-logs-test", &pubkey, 1, Utc::now()).unwrap();

    let res = app.get("/api/bunker/logs").send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["event_id"], "event-for-logs-test");
    assert_eq!(logs[0]["event_kind"], 1);
    assert_eq!(logs[0]["pubkey"], pubkey);
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
