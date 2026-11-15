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
//!
//! A request held for approval (#102) is not awaited here, which would stop every other request:
//! the handler parks it and the relay sends its auth challenge, if any. The real answer is sent
//! later through `RelayHandle::respond`, from the approval routes and the sweep.
use crate::approvals::Scheme;
use crate::nip46::{LaterReply, Nip46Outcome, Nip46Request, Nip46Response};
use crate::signer::Signer;
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
    /// Sends `response` to `client`, encrypted as its request was, signed by the bunker.
    pub async fn respond(&self, signer: &Signer, client: PublicKey, scheme: Scheme, response: &Nip46Response) -> anyhow::Result<()> {
        let response_json = serde_json::to_string(response)?;
        let encrypted = match scheme {
            Scheme::Nip04 => signer.nip04_encrypt(&client, &response_json)?,
            Scheme::Nip44 => signer.encrypt(&client, &response_json)?,
        };
        let response_event = signer.build_event(Kind::from(NIP46_KIND), encrypted, vec![Tag::public_key(client)]).await?;
        self.client.send_event(response_event).await?;
        Ok(())
    }

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
        let scheme = if nip04 { Scheme::Nip04 } else { Scheme::Nip44 };

        let response = match self.state.nip46_handler.handle(request, event.pubkey, scheme).await {
            Nip46Outcome::Reply(response) => response,
            Nip46Outcome::Held { notice: Some(notice), .. } => notice,
            // Held, and the console's address is unknown: the app just waits for the answer.
            Nip46Outcome::Held { notice: None, .. } => return Ok(()),
        };
        let signer = self.state.signer.read().await;
        self.handle.respond(&signer, event.pubkey, scheme, &response).await
    }
}

/// Sends answers decided later (#102) through the running relay client. With none (NIP-46 off,
/// or not started yet) they are logged and dropped: the app gets nothing, as after a restart.
pub async fn deliver(state: &AppState, replies: Vec<LaterReply>) {
    if replies.is_empty() {
        return;
    }
    let Some(handle) = state.relay_handle().await else {
        warn!("{} NIP-46 answers could not be sent: the relay client is not running", replies.len());
        return;
    };
    let signer = state.signer.read().await;
    for reply in replies {
        if let Err(e) = handle.respond(&signer, reply.client_pubkey, reply.scheme, &reply.response).await {
            warn!("Failed to send a NIP-46 answer to {}: {}", reply.client_pubkey, e);
        }
    }
}
