#[path = "../common/mod.rs"]
mod common;

use bunker::db::Database;
use bunker::registry::Role;
use reqwest::Method;
use bunker::server::create_router;
use bunker::state::AppState;
use bunker::config::Config;
use bunker::signer::Signer;
use nostr::prelude::*;
use tokio::net::TcpListener;
use reqwest::StatusCode;
use serde_json::Value;

async fn setup_app() -> (String, reqwest::Client) {
    let (address, client, _) = setup_app_with_bunker_key().await;
    (address, client)
}

/// Also returns the bunker's own npub, which must never be registrable.
async fn setup_app_with_bunker_key() -> (String, reqwest::Client, String) {
    let keys = Keys::generate();
    let bunker_npub = keys.public_key().to_bech32().unwrap();
    let signer = Signer::new(keys.secret_key().clone());
    
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
        proxy_secret: None,
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
    
    (address, client, bunker_npub)
}

fn new_npub() -> String {
    Keys::generate().public_key().to_bech32().unwrap()
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
    let member_keys = Keys::generate();
    let member_pubkey = member_keys.public_key().to_bech32().unwrap();
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
    // Stored and returned as hex, with the npub alongside for display (#24).
    assert_eq!(body[0]["pubkey"], member_keys.public_key().to_hex());
    assert_eq!(body[0]["npub"], member_pubkey);
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
            "pubkey": new_npub(),
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
            "role": "signer"
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
    for (name, pubkey) in [("Alice", new_npub()), ("Bob", new_npub())] {
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

async fn add(address: &str, client: &reqwest::Client, pubkey: &str, role: &str) -> reqwest::Response {
    client
        .post(format!("{}/api/bunker/team", address))
        .json(&serde_json::json!({ "name": "Member", "pubkey": pubkey, "role": role }))
        .send()
        .await
        .expect("Failed to execute request")
}

#[tokio::test]
async fn test_add_team_member_accepts_the_three_roles_only() {
    let (address, client) = setup_app().await;
    for role in ["administrator", "signer", "viewer"] {
        assert_eq!(add(&address, &client, &new_npub(), role).await.status(), StatusCode::OK, "{} is accepted", role);
    }
    // Older names are refused, not translated: pre-#24's `admin` and #24's `user` (#77).
    for role in ["admin", "user", "Admin", "Viewer"] {
        assert_eq!(add(&address, &client, &new_npub(), role).await.status(), StatusCode::BAD_REQUEST, "{} is refused", role);
    }
}

#[tokio::test]
async fn test_add_team_member_rejects_a_prefix_that_is_not_a_key() {
    let (address, client) = setup_app().await;
    // Accepted before #24, which only checked for the npub1 prefix.
    let res = add(&address, &client, "npub1bob0000000000000000000000000000000000000000000000000000000000", "signer").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_add_team_member_accepts_hex_and_refuses_a_duplicate_in_either_form() {
    let (address, client) = setup_app().await;
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();
    let hex = keys.public_key().to_hex();

    assert_eq!(add(&address, &client, &hex, "viewer").await.status(), StatusCode::OK);
    assert_eq!(add(&address, &client, &hex, "signer").await.status(), StatusCode::CONFLICT);
    assert_eq!(add(&address, &client, &npub, "signer").await.status(), StatusCode::CONFLICT, "the same key as an npub");
    assert_eq!(add(&address, &client, &hex.to_uppercase(), "signer").await.status(), StatusCode::CONFLICT, "the same key in upper case");
}

#[tokio::test]
async fn test_add_team_member_refuses_the_bunkers_own_key() {
    let (address, client, bunker_npub) = setup_app_with_bunker_key().await;
    let res = add(&address, &client, &bunker_npub, "administrator").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(res.text().await.unwrap().contains("bunker's own key"));
}

#[tokio::test]
async fn test_lookup_by_pubkey_in_either_form() {
    let (address, client) = setup_app().await;
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();
    let hex = keys.public_key().to_hex();
    assert_eq!(add(&address, &client, &npub, "administrator").await.status(), StatusCode::OK);

    for key in [&npub, &hex] {
        let res = client.get(format!("{}/api/bunker/team/by-pubkey/{}", address, key)).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "found by {}", key);
        let body: Value = res.json().await.unwrap();
        assert_eq!(body["pubkey"], hex);
        assert_eq!(body["npub"], npub);
        assert_eq!(body["role"], "administrator");
    }
}

#[tokio::test]
async fn test_lookup_by_pubkey_misses_clearly() {
    let (address, client) = setup_app().await;

    let res = client.get(format!("{}/api/bunker/team/by-pubkey/{}", address, new_npub())).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"], "not_registered");

    let res = client.get(format!("{}/api/bunker/team/by-pubkey/not-a-key", address)).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_the_last_administrator_cannot_be_removed() {
    // Removing the only administrator would lock everyone out of administration (#25).
    let (address, client) = setup_app().await;
    for role in ["administrator", "signer"] {
        assert_eq!(add(&address, &client, &new_npub(), role).await.status(), StatusCode::OK);
    }
    let team: Vec<Value> = client.get(format!("{}/api/bunker/team", address)).send().await.unwrap().json().await.unwrap();
    let id_of = |role: &str| team.iter().find(|m| m["role"] == role).unwrap()["id"].as_str().unwrap().to_string();

    let res = client.delete(format!("{}/api/bunker/team/{}", address, id_of("administrator"))).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(res.text().await.unwrap().contains("last administrator"));

    // A second administrator makes the first removable.
    assert_eq!(add(&address, &client, &new_npub(), "administrator").await.status(), StatusCode::OK);
    let res = client.delete(format!("{}/api/bunker/team/{}", address, id_of("administrator"))).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

/// The team's access matrix (#77), with the guard on: administrators and viewers read the team
/// and look a member up; only administrators add and remove; signers do neither.
#[tokio::test]
async fn test_the_team_by_role() {
    let app = common::spawn(true).await;
    let signer = app.register(Role::Signer).public_key().to_hex();
    let viewer = app.register(Role::Viewer).public_key().to_hex();
    let admin = app.admin.public_key().to_hex();
    let lookup = format!("/api/bunker/team/by-pubkey/{}", signer);

    for (who, may_read) in [(&admin, true), (&viewer, true), (&signer, false)] {
        let team = app.signed(Method::GET, "/api/bunker/team", who).send().await.unwrap();
        let found = app.signed(Method::GET, &lookup, who).send().await.unwrap();
        if may_read {
            assert_eq!(team.status(), StatusCode::OK);
            let team: Vec<Value> = team.json().await.unwrap();
            let roles: Vec<&str> = team.iter().map(|m| m["role"].as_str().unwrap()).collect();
            assert!(roles.contains(&"administrator") && roles.contains(&"signer") && roles.contains(&"viewer"), "{:?}", roles);
            assert_eq!(found.status(), StatusCode::OK);
            assert_eq!(found.json::<Value>().await.unwrap()["role"], "signer");
        } else {
            assert_eq!(team.status(), StatusCode::FORBIDDEN);
            assert_eq!(found.status(), StatusCode::FORBIDDEN);
        }
    }

    let body = serde_json::json!({ "name": "New", "pubkey": new_npub(), "role": "viewer" });
    for who in [&viewer, &signer] {
        let res = app.signed(Method::POST, "/api/bunker/team", who).json(&body).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    let res = app.signed(Method::POST, "/api/bunker/team", &admin).json(&body).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "an administrator adds a viewer");
    assert_eq!(app.db.get_team_members().unwrap().len(), 4);
}
