use crate::state::AppState;
use crate::nip46::Nip46Request;
use nostr_relay_pool::{RelayPool, RelayPoolOptions, RelayOptions, RelayPoolNotification, SubscribeOptions};
use nostr_sdk::prelude::*;
use tracing::{error, info};

pub struct RelayClient {
    pool: RelayPool,
    state: AppState,
}

impl RelayClient {
    /// Initialize relay connections
    pub async fn new(
        relays: Vec<String>,
        state: AppState,
    ) -> anyhow::Result<Self> {
        let pool = RelayPool::new(RelayPoolOptions::default());
        
        // Connect to each relay
        for url in relays {
            info!("Connecting to relay: {}", url);
            pool.add_relay(url, RelayOptions::default()).await?;
        }

        // So /api/bunker/status can report which relays are connected (#27).
        state.set_relay_pool(pool.clone()).await;

        Ok(Self { pool, state })
    }
    
    /// Start listening for NIP-46 requests
    pub async fn run(&self) -> anyhow::Result<()> {
        // Establish WebSocket connections to all added relays
        self.pool.connect().await;

        // Subscribe to kind 24133 (NIP-46 requests) addressed to our pubkey
        let pubkey_bech32 = self.state.signer.read().await.public_key_bech32();
        let pubkey = PublicKey::from_bech32(&pubkey_bech32)?;
        
        let filter = Filter::new()
            .kind(Kind::from(24133))
            .pubkey(pubkey);
        
        info!("Subscribing to NIP-46 requests for pubkey: {}", pubkey.to_bech32()?);
        self.pool.subscribe(filter, SubscribeOptions::default()).await?;
        
        let mut notifications = self.pool.notifications();
        
        // Process incoming events
        while let Ok(notification) = notifications.recv().await {
            match notification {
                RelayPoolNotification::Event { event, .. } => {
                    info!("Received NIP-46 request event: {}", event.id.to_hex());
                    if let Err(e) = self.handle_nip46_request(*event).await {
                        error!("Error handling NIP-46 request: {}", e);
                    }
                }
                RelayPoolNotification::Message { relay_url, message: RelayMessage::Notice(msg) } => {
                    info!("Relay notice from {}: {}", relay_url, msg);
                }
                _ => {}
            }
        }
        
        Ok(())
    }
    
    /// Handle incoming NIP-46 request
    async fn handle_nip46_request(&self, event: Event) -> anyhow::Result<()> {
        // Verify event kind is 24133
        if event.kind != Kind::from(24133) {
            return Ok(());
        }
        
        // Decrypt content using NIP-44
        // The event content is encrypted by the client for the bunker's pubkey
        let decrypted = self.state.signer.read().await.decrypt(&event.pubkey, &event.content)?;
        
        // Parse JSON-RPC request
        let request: Nip46Request = serde_json::from_str(&decrypted)?;
        
        // Handle request
        let response = self.state.nip46_handler.handle_request(request, event.pubkey).await;
        
        // Encrypt response
        let response_json = serde_json::to_string(&response)?;
        let encrypted = self.state.signer.read().await.encrypt(&event.pubkey, &response_json)?;
        
        // Publish kind 24134 response
        let response_event = self.state.signer.read().await.build_event(
            Kind::from(24134),
            encrypted,
            vec![Tag::public_key(event.pubkey)],
        ).await?;
        
        self.pool.send_event(response_event).await?;
        
        Ok(())
    }
}
