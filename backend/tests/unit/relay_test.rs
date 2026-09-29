use bunker::db::Database;
use bunker::relay::RelayClient;
use bunker::signer::Signer;
use bunker::state::AppState;
use bunker::config::Config;
use nostr::prelude::*;

#[tokio::test]
async fn test_relay_client_new() {
    let keys = Keys::generate();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").unwrap();
    let relays = vec!["ws://relay.threenine.services ".to_string()];
    
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: relays.clone(),
        nip46_enabled: true,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
    };
    let state = AppState::new(signer, db, config);
    let client = RelayClient::new(relays, state).await;
    assert!(client.is_ok());
}

#[tokio::test]
async fn test_relay_client_empty_relays() {
    let keys = Keys::generate();
    let signer = Signer::new(keys.secret_key().clone());
    let db = Database::new(":memory:").unwrap();
    let relays = vec![];
    
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 3000,
        db_path: ":memory:".to_string(),
        relay_urls: relays.clone(),
        nip46_enabled: true,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
    };
    let state = AppState::new(signer, db, config);
    let client = RelayClient::new(relays, state).await;
    assert!(client.is_ok());
}
