// GET /api/bunker/config, through the proxy identity (#25).
#[path = "../common/mod.rs"]
mod common;

use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::Value;

#[tokio::test]
async fn test_get_config() {
    let app = common::spawn(true).await;

    let res = app.get("/api/bunker/config").send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["pubkey"], app.bunker.public_key().to_bech32().unwrap());
    assert!(body.get("nsec").is_none(), "the key is never returned");
    assert!(body["nsec_file"].is_null());
}

#[tokio::test]
async fn test_update_config_is_gone() {
    // The Config page is read-only (#42): the signing key is set with BUNKER_NSEC_FILE or
    // BUNKER_NSEC, and the API no longer accepts a key.
    let app = common::spawn(true).await;

    let res = app
        .request(Method::POST, "/api/bunker/config")
        .json(&serde_json::json!({ "nsec": Keys::generate().secret_key().to_bech32().unwrap() }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}
