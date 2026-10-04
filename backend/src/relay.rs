//! The NIP-46 relay client: listens for requests addressed to the bunker and answers them.
//!
//! Requests and responses are both kind 24133 (NIP-46; #52 found responses sent as 24134, which
//! no standard client reads). Requests may be encrypted with NIP-44, as NIP-46 now specifies and
//! nostr-tools does, or NIP-04, as older clients (rust-nostr 0.39 among them) still do; each is
//! answered in the scheme it used. The client is configured with the bunker's keys, so relays
//! that challenge with NIP-42 AUTH are answered.
//!
//! The relays can change while it runs (#78): an administrator saves a new list in the console and
//! `RelayHandle::set_relays` applies it to the running client, without a restart. With NIP-46 on
//! and no relays yet, the client still starts, so the first list saved takes effect at once.
use crate::nip46::Nip46Request;
use crate::state::AppState;
use nostr_sdk::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// NIP-46 request and response kind.
pub const NIP46_KIND: u16 = 24133;

/// One fixed id for the NIP-46 subscription, so re-subscribing after a relay change replaces it
/// on every relay rather than adding a second one.
const SUBSCRIPTION_ID: &str = "bancwr-nip46";

pub struct RelayClient {
    client: Client,
    state: AppState,
    handle: RelayHandle,
}

/// A cheap, cloneable handle on the running client's relays (#78), kept in `AppState` so
/// `PUT /api/bunker/relays` can change them. It holds the client, not the state, so the state
/// never owns itself.
#[derive(Clone)]
pub struct RelayHandle {
    client: Client,
    bunker: PublicKey,
    /// Serialises changes, so two saves cannot interleave their adds and removes.
    changing: Arc<Mutex<()>>,
}

impl RelayHandle {
    /// Makes the client's relays exactly `urls`: relays no longer listed are removed (and
    /// disconnected), new ones are added and connected, and the NIP-46 subscription is sent again
    /// so every relay carries it. `urls` are normalised already (`bunker_relays::validate`), or
    /// as `NIP46_RELAYS` gave them.
    pub async fn set_relays(&self, urls: &[String]) -> anyhow::Result<()> {
        let _changing = self.changing.lock().await;
        let wanted: Vec<RelayUrl> = urls.iter().map(|url| RelayUrl::parse(url)).collect::<Result<_, _>>()?;
        let current = self.client.relays().await;

        for url in current.keys().filter(|url| !wanted.contains(url)) {
            info!("Removing relay: {}", url);
            // Forced: the bunker uses no gossip relays, and a plain remove would keep one that it did.
            self.client.force_remove_relay(url.clone()).await?;
        }
        for url in wanted.iter().filter(|url| !current.contains_key(*url)) {
            info!("Connecting to relay: {}", url);
            self.client.add_relay(url.clone()).await?;
        }
        // Starts only the relays not yet connecting: the ones just added.
        self.client.connect().await;
        self.subscribe().await
    }

    /// The NIP-46 requests addressed to the bunker, from now on. Kept as the pool's subscription,
    /// which relays added later inherit, so it is saved even while there are no relays to send it
    /// to (the client's `subscribe` would refuse with no relays).
    async fn subscribe(&self) -> anyhow::Result<()> {
        let id = SubscriptionId::new(SUBSCRIPTION_ID);
        let filter = Filter::new().kind(Kind::from(NIP46_KIND)).pubkey(self.bunker).since(Timestamp::now());
        if self.client.relays().await.is_empty() {
            self.client.pool().save_subscription(id, filter).await;
            return Ok(());
        }
        // A relay still connecting has the subscription sent once it connects; only a refusal by
        // every relay is an error.
        if let Err(e) = self.client.subscribe_with_id(id, filter, None).await {
            warn!("Could not send the NIP-46 subscription to any relay yet: {}", e);
        }
        Ok(())
    }
}

impl RelayClient {
    /// Initialize relay connections. `relays` may be empty (#78): the client then waits for a
    /// list saved in the console.
    pub async fn new(relays: Vec<String>, state: AppState) -> anyhow::Result<Self> {
        let keys = state.signer.read().await.keys();
        let bunker = keys.public_key();
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
        // So a list saved in the console applies at once (#78).
        let handle = RelayHandle { client: client.clone(), bunker, changing: Arc::new(Mutex::new(())) };
        state.set_relay_handle(handle.clone()).await;

        Ok(Self { client, state, handle })
    }

    /// The handle `set_relays` is called on.
    pub fn handle(&self) -> RelayHandle {
        self.handle.clone()
    }

    /// As `RelayHandle::set_relays`.
    pub async fn set_relays(&self, urls: &[String]) -> anyhow::Result<()> {
        self.handle.set_relays(urls).await
    }

    /// Start listening for NIP-46 requests
    pub async fn run(&self) -> anyhow::Result<()> {
        self.client.connect().await;

        let pubkey = PublicKey::from_hex(&self.state.signer.read().await.public_key_hex())?;

        info!("Subscribing to NIP-46 requests for pubkey: {}", pubkey.to_bech32()?);
        let mut notifications = self.client.notifications();
        self.handle.subscribe().await?;

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
