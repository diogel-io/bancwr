use bunker::db::Database;
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
const SIGN_PATH: &str = "/sign";

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
    assert_eq!(status.status, "healthy");
    assert_eq!(status.pubkey, expected_pubkey);
    // From the config, which Config::load fills from BANCWR_VERSION (#35).
    assert_eq!(status.version, "1.2.3-test");
}

#[tokio::test]
async fn test_sign_event_handler_success() {
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
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let content = "Test event";
    let unsigned_event = EventBuilder::text_note(content).build(keys.public_key());
    let body = serde_json::to_string(&unsigned_event).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(SIGN_PATH)
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let signed_event: Event = from_slice(&body_bytes).unwrap();
    assert_eq!(signed_event.content, content);
    assert!(signed_event.verify().is_ok());
}

#[tokio::test]
async fn test_sign_event_handler_empty_content() {
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
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let unsigned_event = EventBuilder::text_note("").build(keys.public_key());
    let body = serde_json::to_string(&unsigned_event).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(SIGN_PATH)
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert!(body_str.contains("Content cannot be empty"));
}

#[tokio::test]
async fn test_sign_event_handler_forbidden_kind() {
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
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let unsigned_event = UnsignedEvent {
        id: None,
        pubkey: keys.public_key(),
        created_at: Timestamp::now(),
        kind: Kind::from(0),
        tags: Tags::new(vec![]),
        content: "Profile update".to_string(),
    };
    let body = serde_json::to_string(&unsigned_event).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(SIGN_PATH)
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert!(body_str.contains("Kind 0 not allowed"));
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
