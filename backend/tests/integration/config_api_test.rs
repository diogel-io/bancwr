use bunker::db::Database;
use bunker::server::create_router;
use bunker::state::AppState;
use bunker::config::Config;
use bunker::signer::Signer;
use nostr::prelude::*;
use tokio::net::TcpListener;
use reqwest::StatusCode;
use serde_json::Value;

async fn setup_app() -> (String, reqwest::Client, Keys) {
    let keys = Keys::generate();
    let signer = Signer::new(keys.secret_key().clone());
    
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
    };
    let state = AppState::new(signer, db, config);
    let app = create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind random port");
    let addr = listener.local_addr().expect("Failed to get local address");
    let port = addr.port();

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("SERVER ERROR IN TEST: {:?}", e);
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let address = format!("http://127.0.0.1:{}", port);
    let client = reqwest::Client::new();
    
    (address, client, keys)
}

#[tokio::test]
async fn test_get_config() {
    let (address, client, keys) = setup_app().await;

    let res = client
        .get(format!("{}/api/bunker/config", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body["pubkey"], keys.public_key().to_bech32().unwrap());
    assert!(body["nsec"].is_null()); // Note: this might fail if nsec is missing instead of null
    assert!(body["nsec_file"].is_null());
}

#[tokio::test]
async fn test_update_config_is_gone() {
    // The Config page is read-only (#42): the signing key is set with BUNKER_NSEC_FILE or
    // BUNKER_NSEC, and the API no longer accepts a key.
    let (address, client, _keys) = setup_app().await;

    let res = client
        .post(format!("{}/api/bunker/config", address))
        .json(&serde_json::json!({ "nsec": Keys::generate().secret_key().to_bech32().unwrap() }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}
