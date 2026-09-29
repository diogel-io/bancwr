use bunker::db::Database;
use bunker::server::create_router;
use bunker::state::AppState;
use bunker::config::Config;
use bunker::signer::Signer;
use nostr::prelude::*;
use tokio::net::TcpListener;
use reqwest::StatusCode;
use serde_json::Value;

async fn setup_app() -> (String, PublicKey, reqwest::Client) {
    let keys = Keys::generate();
    let pubkey = keys.public_key();
    let signer = Signer::new(keys.secret_key().clone());
    
    // In-memory SQLite for tests
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    // Bind to 127.0.0.1 on a random port
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind random port");
    let addr = listener.local_addr().expect("Failed to get local address");
    let port = addr.port();

    // Start server in background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("SERVER ERROR IN TEST: {:?}", e);
        }
    });

    // Give the server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let address = format!("http://127.0.0.1:{}", port);
    let client = reqwest::Client::new();
    
    (address, pubkey, client)
}

#[tokio::test]
async fn test_api_health_check() {
    let (address, _, client) = setup_app().await;

    let res = client
        .get(format!("{}/health", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.expect("Failed to parse JSON");
    // Exactly this and nothing more: /health stays a cheap container probe. The version is on
    // /api/bunker/status instead (#35).
    assert_eq!(body, serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn test_api_status() {
    let (address, expected_pubkey, client) = setup_app().await;

    let res = client
        .get(format!("{}/api/bunker/status", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body["status"], "healthy");
    assert_eq!(body["pubkey"], expected_pubkey.to_bech32().unwrap());
    assert_eq!(body["version"], "0.0.0");
}

#[tokio::test]
async fn test_api_sign_event_success() {
    let (address, bunker_pubkey, client) = setup_app().await;
    
    // The event to be signed should have the bunker's public key
    let content = "Integration test note";
    let unsigned_event = EventBuilder::text_note(content).build(bunker_pubkey);

    let res = client
        .post(format!("{}/sign", address))
        .json(&unsigned_event)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let signed_event: Event = res.json().await.expect("Failed to parse signed event");
    assert_eq!(signed_event.content, content);
    assert_eq!(signed_event.pubkey, bunker_pubkey);
    assert!(signed_event.verify().is_ok());
}

#[tokio::test]
async fn test_api_sign_event_forbidden_kind() {
    let (address, bunker_pubkey, client) = setup_app().await;
    
    let unsigned_event = UnsignedEvent {
        id: None,
        pubkey: bunker_pubkey,
        created_at: Timestamp::now(),
        kind: Kind::from(0),
        tags: Tags::new(vec![]),
        content: "Forbidden profile update".to_string(),
    };

    let res = client
        .post(format!("{}/sign", address))
        .json(&unsigned_event)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let error_text = res.text().await.expect("Failed to get error text");
    assert!(error_text.contains("Kind 0 not allowed"));
}

#[tokio::test]
async fn test_api_sign_event_invalid_json() {
    let (address, _, client) = setup_app().await;
    
    let res = client
        .post(format!("{}/sign", address))
        .header("Content-Type", "application/json")
        .body("{\"invalid\": \"json\"}")
        .send()
        .await
        .expect("Failed to execute request");

    // Axum returns 422 Unprocessable Entity for missing fields in JSON extractor
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_api_get_logs() {
    let (address, bunker_pubkey, client) = setup_app().await;
    
    // 1. Get initial log count
    let res = client
        .get(format!("{}/api/bunker/logs", address))
        .send()
        .await
        .expect("Failed to execute request");
    
    assert_eq!(res.status(), StatusCode::OK);
    let initial_logs: Vec<Value> = res.json().await.expect("Failed to parse logs");

    // 2. Sign an event to create a new log
    let content = format!("Log test note {}", Timestamp::now().as_u64());
    let unsigned_event = EventBuilder::text_note(content).build(bunker_pubkey);
    
    client
        .post(format!("{}/sign", address))
        .json(&unsigned_event)
        .send()
        .await
        .expect("Failed to execute request");

    // 3. Check logs again
    let res = client
        .get(format!("{}/api/bunker/logs", address))
        .send()
        .await
        .expect("Failed to execute request");
    
    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.expect("Failed to parse logs");
    
    // Use >= because other tests might be signing events in parallel
    assert!(logs.len() > initial_logs.len());
    
    // Check if any of the top logs matches our expected event
    let found = logs.iter().take(10).any(|l| {
        l["event_kind"] == 1 && l["pubkey"] == bunker_pubkey.to_bech32().unwrap()
    });
    assert!(found, "Our signed event log not found in top logs");
}

#[tokio::test]
async fn test_api_metrics() {
    let (address, _, client) = setup_app().await;

    // 1. Initial metrics
    let res = client
        .get(format!("{}/api/bunker/metrics", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let initial_metrics: Value = res.json().await.expect("Failed to parse metrics");
    let initial_requests = initial_metrics["http_requests"].as_u64().unwrap();

    // 2. Do something that increments request count (e.g., call status)
    client
        .get(format!("{}/api/bunker/status", address))
        .send()
        .await
        .expect("Failed to execute request");

    // 3. Check metrics again
    let res = client
        .get(format!("{}/api/bunker/metrics", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let metrics: Value = res.json().await.expect("Failed to parse metrics");
    let requests = metrics["http_requests"].as_u64().unwrap();
    
    // It should have incremented by at least 1 (the status call)
    // and maybe more depending on other concurrent tests
    assert!(requests > initial_requests);
}
