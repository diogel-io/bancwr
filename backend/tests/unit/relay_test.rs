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
        proxy_secret: None,
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
        proxy_secret: None,
    };
    let state = AppState::new(signer, db, config);
    let client = RelayClient::new(relays, state).await;
    assert!(client.is_ok());
}

// set_relays (#78): an administrator's new list applies to the running client without a restart.
mod set_relays {
    use super::*;
    use bunker::relay::NIP46_KIND;
    use nostr_relay_builder::MockRelay;
    use nostr_sdk::{Client, RelayPoolNotification, RelayStatus};
    use std::time::Duration;

    /// NIP-46 on, with no relays from NIP46_RELAYS: as a bunker managed from the console starts.
    fn state() -> (AppState, Keys) {
        let keys = Keys::generate();
        let config = Config {
            secret_key: keys.secret_key().clone(),
            port: 0,
            db_path: ":memory:".to_string(),
            relay_urls: vec![],
            nip46_enabled: true,
            nsec_file: None,
            version: "0.0.0".to_string(),
            admin_pubkey: None,
            proxy_secret: None,
        };
        (AppState::new(Signer::new(keys.secret_key().clone()), Database::new(":memory:").unwrap(), config), keys)
    }

    /// The relays the pool holds, and which of them are connected, once `done` holds (or 10 s).
    async fn pool_until(state: &AppState, done: impl Fn(&[(String, bool)]) -> bool) -> Vec<(String, bool)> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let pool = state.relay_pool().await.expect("the client registered its pool");
            let mut relays: Vec<(String, bool)> = pool
                .relays()
                .await
                .into_iter()
                .map(|(url, relay)| (url.to_string(), relay.status() == RelayStatus::Connected))
                .collect();
            relays.sort();
            if done(&relays) || tokio::time::Instant::now() > deadline {
                return relays;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Whether a NIP-46 request sent through `relay` is answered by the bunker: proof the
    /// subscription reached that relay. Unconnected, so the answer is an error, but an answer.
    async fn answered_through(relay: &str, bunker: PublicKey) -> bool {
        let keys = Keys::generate();
        let client = Client::builder().signer(keys.clone()).build();
        client.add_relay(relay).await.unwrap();
        client.connect().await;
        client.wait_for_connection(Duration::from_secs(5)).await;
        client.subscribe(Filter::new().kind(Kind::from(NIP46_KIND)).pubkey(keys.public_key()), None).await.unwrap();
        let mut notifications = client.notifications();
        let body = serde_json::json!({ "id": "p", "method": "ping", "params": [] }).to_string();
        let content = nip44::encrypt(keys.secret_key(), &bunker, body, nip44::Version::V2).unwrap();
        let request = EventBuilder::new(Kind::from(NIP46_KIND), content).tag(Tag::public_key(bunker)).sign_with_keys(&keys).unwrap();
        client.send_event(request).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Ok(notification) = notifications.recv().await {
                if let RelayPoolNotification::Event { event, .. } = notification {
                    if event.kind == Kind::from(NIP46_KIND) && event.pubkey == bunker {
                        return true;
                    }
                }
            }
            false
        })
        .await
        .unwrap_or(false)
    }

    #[tokio::test]
    async fn adds_and_removes_relays_on_a_running_client_and_answers_through_the_new_ones() {
        let first = MockRelay::run().await.unwrap();
        let second = MockRelay::run().await.unwrap();
        let (state, keys) = state();

        // Started with no relays (NIP-46 on, nothing configured yet): it must still run.
        let client = RelayClient::new(vec![], state.clone()).await.unwrap();
        let handle = client.handle();
        let running = tokio::spawn(async move { client.run().await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!running.is_finished(), "the client keeps running with no relays");
        assert!(state.relay_handle().await.is_some(), "the handle is in the state for the API");

        handle.set_relays(&[first.url()]).await.unwrap();
        let relays = pool_until(&state, |r| r.iter().all(|(_, up)| *up)).await;
        assert_eq!(relays, vec![(first.url(), true)]);
        assert!(answered_through(&first.url(), keys.public_key()).await, "subscribed on the relay added");

        // Replaced: the first is dropped, the second added.
        handle.set_relays(&[second.url()]).await.unwrap();
        let relays = pool_until(&state, |r| r.len() == 1 && r.iter().all(|(_, up)| *up)).await;
        assert_eq!(relays, vec![(second.url(), true)]);
        assert!(answered_through(&second.url(), keys.public_key()).await, "subscribed on the relay that replaced it");
        assert!(!answered_through(&first.url(), keys.public_key()).await, "no longer listening on the relay removed");

        // Both, then none.
        handle.set_relays(&[first.url(), second.url()]).await.unwrap();
        let relays = pool_until(&state, |r| r.len() == 2 && r.iter().all(|(_, up)| *up)).await;
        assert_eq!(relays.len(), 2);
        handle.set_relays(&[]).await.unwrap();
        assert!(pool_until(&state, |r| r.is_empty()).await.is_empty());
        assert!(!running.is_finished());
    }
}
