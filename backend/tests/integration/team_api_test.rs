use bunker::db::Database;
use bunker::server::create_router;
use bunker::state::AppState;
use bunker::config::Config;
use bunker::signer::Signer;
use nostr::prelude::*;
use tokio::net::TcpListener;
use reqwest::StatusCode;
use serde_json::Value;

async fn setup_app() -> (String, reqwest::Client) {
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
    
    (address, client)
}

#[tokio::test]
async fn test_team_management_flow() {
    let (address, client) = setup_app().await;

    // 1. Initially team should be empty
    let res = client
        .get(format!("{}/api/bunker/team", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Vec<Value> = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body.len(), 0);

    // 2. Add a team member
    let member_name = "Alice";
    let member_pubkey = "npub1663u3p9a7lcs64a5940u3l9j764a5940u3l9j764a5940u3l9j764a5940u3"; // Mock npub
    let member_role = "signer";

    let res = client
        .post(format!("{}/api/bunker/team", address))
        .json(&serde_json::json!({
            "name": member_name,
            "pubkey": member_pubkey,
            "role": member_role
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body["success"], true);

    // 3. Get team members and verify
    let res = client
        .get(format!("{}/api/bunker/team", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::OK);
    let body: Vec<Value> = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], member_name);
    assert_eq!(body[0]["pubkey"], member_pubkey);
    assert_eq!(body[0]["role"], member_role);
    assert!(body[0]["id"].is_string());
}

#[tokio::test]
async fn test_add_team_member_invalid_role() {
    let (address, client) = setup_app().await;

    let res = client
        .post(format!("{}/api/bunker/team", address))
        .json(&serde_json::json!({
            "name": "Bob",
            "pubkey": "npub1663u3p9a7lcs64a5940u3l9j764a5940u3l9j764a5940u3l9j764a5940u3",
            "role": "hacker"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_add_team_member_invalid_pubkey_format() {
    let (address, client) = setup_app().await;

    let res = client
        .post(format!("{}/api/bunker/team", address))
        .json(&serde_json::json!({
            "name": "Charlie",
            "pubkey": "invalid-pubkey",
            "role": "viewer"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_remove_team_member() {
    let (address, client) = setup_app().await;

    // Add two members, so removing one can be shown to leave the other.
    for (name, pubkey) in [
        ("Alice", "npub1663u3p9a7lcs64a5940u3l9j764a5940u3l9j764a5940u3l9j764a5940u3"),
        ("Bob", "npub1bob0000000000000000000000000000000000000000000000000000000000"),
    ] {
        let res = client
            .post(format!("{}/api/bunker/team", address))
            .json(&serde_json::json!({ "name": name, "pubkey": pubkey, "role": "signer" }))
            .send()
            .await
            .expect("Failed to execute request");
        assert_eq!(res.status(), StatusCode::OK);
    }

    let team: Vec<Value> = client
        .get(format!("{}/api/bunker/team", address))
        .send()
        .await
        .expect("Failed to execute request")
        .json()
        .await
        .expect("Failed to parse JSON");
    let alice = team.iter().find(|m| m["name"] == "Alice").expect("Alice is listed");
    let alice_id = alice["id"].as_str().expect("id is a string");

    // Remove Alice by her id
    let res = client
        .delete(format!("{}/api/bunker/team/{}", address, alice_id))
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body["success"], true);

    // Only Bob remains
    let team: Vec<Value> = client
        .get(format!("{}/api/bunker/team", address))
        .send()
        .await
        .expect("Failed to execute request")
        .json()
        .await
        .expect("Failed to parse JSON");
    assert_eq!(team.len(), 1);
    assert_eq!(team[0]["name"], "Bob");

    // Removing Alice again finds nothing
    let res = client
        .delete(format!("{}/api/bunker/team/{}", address, alice_id))
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_remove_team_member_unknown_id() {
    let (address, client) = setup_app().await;

    let res = client
        .delete(format!("{}/api/bunker/team/{}", address, uuid::Uuid::new_v4()))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_remove_team_member_invalid_id() {
    let (address, client) = setup_app().await;

    // A table row index, which is what the frontend used to send (#43)
    let res = client
        .delete(format!("{}/api/bunker/team/0", address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
