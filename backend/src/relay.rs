//! The NIP-46 relay client: listens for requests addressed to the bunker and answers them.
//!
//! Requests and responses are both kind 24133 (NIP-46; #52 found responses sent as 24134, which
//! no standard client reads). Requests may be encrypted with NIP-44, as NIP-46 now specifies and
//! nostr-tools does, or NIP-04, as older clients (rust-nostr 0.39 among them) still do; each is
//! answered in the scheme it used. The client is configured with the bunker's keys, so relays
//! that challenge with NIP-42 AUTH are answered.
use crate::nip46::Nip46Request;
use crate::state::AppState;
use nostr_sdk::prelude::*;
use tracing::{error, info, warn};

/// NIP-46 request and response kind.
pub const NIP46_KIND: u16 = 24133;

pub struct RelayClient {
    client: Client,
    state: AppState,
}

impl RelayClient {
    /// Initialize relay connections
    pub async fn new(relays: Vec<String>, state: AppState) -> anyhow::Result<Self> {
        let keys = state.signer.read().await.keys();
        let client = Client::builder()
            .signer(keys)
            .opts(Options::new().automatic_authentication(true))
            .build();

        for url in relays {
            info!("Connecting to relay: {}", url);
            client.add_relay(url).await?;
        }

        // So /api/bunker/status can report which relays are connected (#27).
        state.set_relay_pool(client.pool().clone()).await;

        Ok(Self { client, state })
    }

    /// Start listening for NIP-46 requests
    pub async fn run(&self) -> anyhow::Result<()> {
        self.client.connect().await;

        let pubkey = PublicKey::from_hex(&self.state.signer.read().await.public_key_hex())?;
        let filter = Filter::new().kind(Kind::from(NIP46_KIND)).pubkey(pubkey).since(Timestamp::now());

        info!("Subscribing to NIP-46 requests for pubkey: {}", pubkey.to_bech32()?);
        let mut notifications = self.client.notifications();
        self.client.subscribe(filter, None).await?;

        while let Ok(notification) = notifications.recv().await {
            match notification {
                RelayPoolNotification::Event { event, .. } => {
                    if let Err(e) = self.handle_nip46_request(*event, pubkey).await {
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

    /// Answers one request. Only kind 24133 events from someone else, addressed to the bunker and
    /// readable with its key, are requests: its own responses (the same kind) are ignored.
    async fn handle_nip46_request(&self, event: Event, bunker: PublicKey) -> anyhow::Result<()> {
        if event.kind != Kind::from(NIP46_KIND) || event.pubkey == bunker {
            return Ok(());
        }
        if !event.tags.public_keys().any(|p| *p == bunker) {
            return Ok(());
        }
        // NIP-04 ciphertext carries "?iv=" (as nostr-connect tells them apart).
        let nip04 = event.content.contains("?iv=");
        let decrypted = {
            let signer = self.state.signer.read().await;
            if nip04 { signer.nip04_decrypt(&event.pubkey, &event.content) } else { signer.decrypt(&event.pubkey, &event.content) }
        };
        let decrypted = match decrypted {
            Ok(decrypted) => decrypted,
            Err(e) => {
                warn!("Ignoring a NIP-46 event from {} that could not be decrypted: {}", event.pubkey, e);
                return Ok(());
            }
        };
        info!("Received NIP-46 request event: {}", event.id.to_hex());
        let request: Nip46Request = serde_json::from_str(&decrypted)?;

        let response = self.state.nip46_handler.handle_request(request, event.pubkey).await;

        let response_json = serde_json::to_string(&response)?;
        let encrypted = {
            let signer = self.state.signer.read().await;
            if nip04 { signer.nip04_encrypt(&event.pubkey, &response_json)? } else { signer.encrypt(&event.pubkey, &response_json)? }
        };
        let response_event = self
            .state
            .signer
            .read()
            .await
            .build_event(Kind::from(NIP46_KIND), encrypted, vec![Tag::public_key(event.pubkey)])
            .await?;

        self.client.send_event(response_event).await?;
        Ok(())
    }
}
