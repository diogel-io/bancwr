use bunker::db::Database;
use bunker::health::{CheckStatus, Overall};
use bunker::server::{create_router, BunkerStatus, HealthResponse};
use bunker::state::AppState;
use bunker::config::Config;
use bunker::signer::Signer;
use chrono::Utc;
use nostr::prelude::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt; // for `oneshot`
use serde_json::from_slice;

const HEALTH_PATH: &str = "/health";
const STATUS_PATH: &str = "/api/bunker/status";

#[tokio::test]
async fn test_health_check_handler() {
    let keys = Keys::generate();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(HEALTH_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let health: HealthResponse = from_slice(&body).unwrap();
    assert_eq!(health.status, "ok");
}

#[tokio::test]
async fn test_status_handler() {
    let keys = Keys::generate();
    let expected_pubkey = keys.public_key().to_bech32().unwrap();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
        version: "1.2.3-test".to_string(),
        admin_pubkey: None,
        proxy_secret: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(STATUS_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let status: BunkerStatus = from_slice(&body).unwrap();
    // NIP-46 is off here, so the relay check is disabled and the signer and database decide (#27).
    assert_eq!(status.status, Overall::Healthy);
    let names: Vec<&str> = status.checks.iter().map(|check| check.name.as_str()).collect();
    assert_eq!(names, ["signer", "database", "relays"]);
    assert_eq!(status.checks[2].status, CheckStatus::Disabled);
    assert_eq!(status.pubkey, expected_pubkey);
    // From the config, which Config::load fills from BANCWR_VERSION (#35).
    assert_eq!(status.version, "1.2.3-test");
}

#[tokio::test]
async fn test_get_logs_handler() {
    let keys = Keys::generate();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    
    // Log an event manually
    db.log_signing_event("event_id_1", "pubkey_1", 1, Utc::now()).unwrap();
    
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/bunker/logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let logs: Vec<bunker::server::LogEntry> = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].event_id, "event_id_1");
    assert_eq!(logs[0].pubkey, "pubkey_1");
    assert_eq!(logs[0].event_kind, 1);
}

#[tokio::test]
async fn test_get_config_handler() {
    let keys = Keys::generate();
    let expected_pubkey = keys.public_key().to_bech32().unwrap();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: Some("/tmp/nsec".to_string()),
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/bunker/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let config_res: bunker::server::ConfigResponse = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(config_res.pubkey, expected_pubkey);
    assert_eq!(config_res.nsec_file, Some("/tmp/nsec".to_string()));
}
