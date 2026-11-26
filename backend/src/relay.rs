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
//! A bunker may hold several identities (#103). The subscription asks for requests to any of them,
//! and each request is decrypted, handled and answered as the identity its `p` tag names. When an
//! administrator creates or retires one (#146), `RelayHandle::set_identities` sends the
//! subscription again for the new set, on the running client.
//!
//! A request held for approval (#102) is not awaited here, which would stop every other request:
//! the handler parks it and the relay sends its auth challenge, if any. The real answer is sent
//! later through `RelayHandle::respond`, from the approval routes and the sweep.
//!
//! An app connected from its own `nostrconnect://` code (#148) chose its relays, which may not be
//! the bunker's. While its connection lasts the bunker also listens there, for that app's requests
//! only, and answers there as well as on its own relays. Those relays are held by a second client,
//! `apps`, with no signer and NIP-42 AUTH off: the bunker authenticates only to relays an
//! administrator chose, and an app's relay learns nothing but the NIP-46 traffic it carries
//! anyway (B4). `sync_app_relays` makes them match the active connections; a relay that is also
//! one of the bunker's is used through the bunker's own client. A request that reaches the bunker
//! through both clients is handled once.
use crate::approvals::Scheme;
use crate::nip46::{LaterReply, Nip46Outcome, Nip46Request, Nip46Response};
use crate::signer::Signer;
use crate::state::AppState;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// NIP-46 request and response kind.
pub const NIP46_KIND: u16 = 24133;

/// One fixed id for the NIP-46 subscription, so re-subscribing after a relay change replaces it
/// on every relay rather than adding a second one.
const SUBSCRIPTION_ID: &str = "bancwr-nip46";
/// The subscription on apps' own relays (#148), for the requests of the apps routed there.
const APP_SUBSCRIPTION_ID: &str = "bancwr-nip46-apps";
/// How long connecting to an app's relay may take before the `connect` response is sent anyway.
const APP_RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How many recent request ids are remembered, so one that arrives through both clients is
/// handled once.
const SEEN_EVENTS: usize = 4096;

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
    /// Every identity the bunker signs as (#103): requests to any of them are answered. Changes
    /// while the bunker runs (#146); the lock is never held across an await.
    identities: Arc<std::sync::RwLock<Vec<PublicKey>>>,
    /// Serialises changes, so two saves cannot interleave their adds and removes.
    changing: Arc<Mutex<()>>,
    /// Apps' own relays (#148): no signer, and no NIP-42 AUTH.
    apps: Client,
    /// Each app connected from a `nostrconnect://` code, with the relays it chose. The lock is never
    /// held across an await.
    routes: Arc<std::sync::RwLock<HashMap<PublicKey, Vec<RelayUrl>>>>,
    /// Recent request ids, oldest first.
    seen: Arc<std::sync::Mutex<(HashSet<EventId>, VecDeque<EventId>)>>,
}

impl RelayHandle {
    fn new(client: Client, identities: Vec<PublicKey>) -> Self {
        let apps = Client::builder().opts(Options::new().automatic_authentication(false)).build();
        Self {
            client,
            identities: Arc::new(std::sync::RwLock::new(identities)),
            changing: Arc::new(Mutex::new(())),
            apps,
            routes: Arc::new(std::sync::RwLock::new(HashMap::new())),
            seen: Arc::new(std::sync::Mutex::new((HashSet::new(), VecDeque::new()))),
        }
    }

    /// Sends `response` to `client`, encrypted as its request was, signed by `signer`: the identity
    /// the request was addressed to. It goes to the bunker's relays and, for an app connected from
    /// its own code (#148), to that app's relays too. An error only when no relay took it.
    pub async fn respond(&self, signer: &Signer, client: PublicKey, scheme: Scheme, response: &Nip46Response) -> anyhow::Result<()> {
        let response_json = serde_json::to_string(response)?;
        let encrypted = match scheme {
            Scheme::Nip04 => signer.nip04_encrypt(&client, &response_json)?,
            Scheme::Nip44 => signer.encrypt(&client, &response_json)?,
        };
        let response_event = signer.build_event(Kind::from(NIP46_KIND), encrypted, vec![Tag::public_key(client)]).await?;
        let own = self.client.send_event(response_event.clone()).await;
        let app_relays = self.route(&client);
        if app_relays.is_empty() {
            own?;
            return Ok(());
        }
        let theirs = self.send_to(&app_relays, response_event).await;
        match (own, theirs) {
            (Ok(_), _) | (_, Ok(())) => Ok(()),
            (Err(e), Err(_)) => Err(e.into()),
        }
    }

    /// Sends the `connect` response for an app's `nostrconnect://` code (#148): from `signer`, the
    /// identity the member chose, to `client`, NIP-44 encrypted, with `secret` as its result and a
    /// fresh id, on the app's `relays` only. An error when none of them took it.
    pub async fn send_connect(&self, signer: &Signer, client: PublicKey, secret: &str, relays: &[String]) -> anyhow::Result<()> {
        let response = Nip46Response { id: uuid::Uuid::new_v4().simple().to_string(), result: Some(secret.to_string()), error: None };
        let encrypted = signer.encrypt(&client, &serde_json::to_string(&response)?)?;
        let event = signer.build_event(Kind::from(NIP46_KIND), encrypted, vec![Tag::public_key(client)]).await?;
        let relays: Vec<RelayUrl> = relays.iter().map(|url| RelayUrl::parse(url)).collect::<Result<_, _>>()?;
        self.send_to(&relays, event).await
    }

    /// Sends `event` to `relays`: through the bunker's own client for those it holds, and through
    /// `apps` for the rest. Ok when at least one took it.
    async fn send_to(&self, relays: &[RelayUrl], event: Event) -> anyhow::Result<()> {
        let own = self.client.relays().await;
        let (shared, theirs): (Vec<RelayUrl>, Vec<RelayUrl>) = relays.iter().cloned().partition(|url| own.contains_key(url));
        let mut sent = false;
        let mut last_error = None;
        if !shared.is_empty() {
            match self.client.send_event_to(shared, event.clone()).await {
                Ok(_) => sent = true,
                Err(e) => last_error = Some(anyhow::Error::from(e)),
            }
        }
        if !theirs.is_empty() {
            match self.apps.send_event_to(theirs, event).await {
                Ok(_) => sent = true,
                Err(e) => last_error = Some(anyhow::Error::from(e)),
            }
        }
        match (sent, last_error) {
            (true, _) => Ok(()),
            (false, Some(e)) => Err(e),
            (false, None) => Err(anyhow::anyhow!("no relays to send to")),
        }
    }

    /// The relays `client` chose (#148), if it connected from its own code.
    fn route(&self, client: &PublicKey) -> Vec<RelayUrl> {
        self.routes.read().unwrap_or_else(|poisoned| poisoned.into_inner()).get(client).cloned().unwrap_or_default()
    }

    /// Whether `client` connected from its own code, so may be heard on apps' relays.
    fn is_routed(&self, client: &PublicKey) -> bool {
        self.routes.read().unwrap_or_else(|poisoned| poisoned.into_inner()).contains_key(client)
    }

    /// Whether `id` was seen before; remembers it either way.
    fn seen_before(&self, id: EventId) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (set, order) = &mut *seen;
        if !set.insert(id) {
            return true;
        }
        order.push_back(id);
        if order.len() > SEEN_EVENTS {
            if let Some(oldest) = order.pop_front() {
                set.remove(&oldest);
            }
        }
        false
    }

    /// Makes the apps' relays exactly those of `routes` (#148): each app connected from its own
    /// code, with the relays it chose. Relays no app needs any more are dropped, new ones are
    /// connected (waiting a few seconds for each, so a `connect` response sent next can reach it),
    /// and the apps' subscription is sent again for the new set.
    pub async fn set_routes(&self, routes: Vec<(PublicKey, Vec<String>)>) -> anyhow::Result<()> {
        let _changing = self.changing.lock().await;
        let mut parsed: HashMap<PublicKey, Vec<RelayUrl>> = HashMap::new();
        for (client, relays) in routes {
            let urls: Vec<RelayUrl> = relays.iter().filter_map(|url| RelayUrl::parse(url).ok()).collect();
            if !urls.is_empty() {
                parsed.insert(client, urls);
            }
        }
        *self.routes.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = parsed;
        self.apply_routes().await
    }

    /// Connects `apps` to the relays the routes need that the bunker's own client does not hold,
    /// and subscribes there. Called with `changing` held.
    async fn apply_routes(&self) -> anyhow::Result<()> {
        let own = self.client.relays().await;
        let routes = self.routes.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        let wanted: HashSet<RelayUrl> = routes.values().flatten().filter(|url| !own.contains_key(*url)).cloned().collect();
        let current = self.apps.relays().await;
        for url in current.keys().filter(|url| !wanted.contains(*url)) {
            info!("Leaving an app's relay: {}", url);
            self.apps.force_remove_relay(url.clone()).await?;
        }
        for url in wanted.iter().filter(|url| !current.contains_key(*url)) {
            info!("Connecting to an app's relay: {}", url);
            self.apps.add_relay(url.clone()).await?;
            if let Err(e) = self.apps.try_connect_relay(url.clone(), APP_RELAY_CONNECT_TIMEOUT).await {
                warn!("Could not connect to the app's relay {} yet: {}", url, e);
            }
        }
        self.subscribe_apps().await
    }

    /// The requests of the routed apps, to any identity, on the apps' relays. Nothing when there are
    /// none.
    async fn subscribe_apps(&self) -> anyhow::Result<()> {
        let authors: Vec<PublicKey> = self.routes.read().unwrap_or_else(|poisoned| poisoned.into_inner()).keys().copied().collect();
        let id = SubscriptionId::new(APP_SUBSCRIPTION_ID);
        if authors.is_empty() || self.apps.relays().await.is_empty() {
            self.apps.unsubscribe_all().await;
            return Ok(());
        }
        let filter = Filter::new().kind(Kind::from(NIP46_KIND)).authors(authors).pubkeys(self.identities()).since(Timestamp::now());
        if let Err(e) = self.apps.subscribe_with_id(id, filter, None).await {
            warn!("Could not send the NIP-46 subscription to any app's relay yet: {}", e);
        }
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
        self.subscribe().await?;
        // A relay the bunker now holds, or no longer does, may be one an app chose (#148).
        self.apply_routes().await
    }

    /// Makes the identities the client listens for exactly `identities` (#146), and sends the
    /// subscription again so every relay carries the new set at once.
    pub async fn set_identities(&self, identities: Vec<PublicKey>) -> anyhow::Result<()> {
        let _changing = self.changing.lock().await;
        *self.identities.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = identities;
        self.subscribe().await?;
        self.subscribe_apps().await
    }

    /// The identities the client listens for now.
    pub fn identities(&self) -> Vec<PublicKey> {
        self.identities.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    /// The NIP-46 requests addressed to the bunker, from now on. Kept as the pool's subscription,
    /// which relays added later inherit, so it is saved even while there are no relays to send it
    /// to (the client's `subscribe` would refuse with no relays).
    async fn subscribe(&self) -> anyhow::Result<()> {
        let id = SubscriptionId::new(SUBSCRIPTION_ID);
        let filter = Filter::new().kind(Kind::from(NIP46_KIND)).pubkeys(self.identities()).since(Timestamp::now());
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
        let identities = state.keyring.public_keys().await;
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
        let handle = RelayHandle::new(client.clone(), identities);
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

        for identity in &self.handle.identities() {
            info!("Subscribing to NIP-46 requests for: {}", identity.to_bech32()?);
        }
        let mut notifications = self.client.notifications();
        let mut app_notifications = self.handle.apps.notifications();
        self.handle.subscribe().await?;
        // Apps that connected from their own codes before a restart (#148).
        sync_app_relays(&self.state).await;

        let mut apps_open = true;
        loop {
            let (notification, from_app_relay) = tokio::select! {
                received = notifications.recv() => match received {
                    Ok(notification) => (notification, false),
                    Err(RecvError::Lagged(missed)) => {
                        warn!("The NIP-46 relay client fell behind and missed {} notifications", missed);
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                },
                received = app_notifications.recv(), if apps_open => match received {
                    Ok(notification) => (notification, true),
                    Err(RecvError::Lagged(missed)) => {
                        warn!("The apps' relay client fell behind and missed {} notifications", missed);
                        continue;
                    }
                    Err(RecvError::Closed) => {
                        apps_open = false;
                        continue;
                    }
                },
            };
            match notification {
                RelayPoolNotification::Event { event, .. } => {
                    if let Err(e) = self.handle_nip46_request(*event, from_app_relay).await {
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

    /// Answers one request. Only kind 24133 events from someone else, addressed to one of the
    /// bunker's identities and readable with its key, are requests: the bunker's own responses (the
    /// same kind) are ignored. On an app's relay (#148), only the routed apps are heard, whatever
    /// the relay sends; and a request already handled through the other client is not handled again.
    async fn handle_nip46_request(&self, event: Event, from_app_relay: bool) -> anyhow::Result<()> {
        if event.kind != Kind::from(NIP46_KIND) || self.state.keyring.identity_of(&event.pubkey).await.is_some() {
            return Ok(());
        }
        if from_app_relay && !self.handle.is_routed(&event.pubkey) {
            return Ok(());
        }
        if self.handle.seen_before(event.id) {
            return Ok(());
        }
        // The identity addressed (#103): the first `p` tag that is one of the bunker's.
        let mut addressed = None;
        for p in event.tags.public_keys() {
            if let Some(identity) = self.state.keyring.identity_of(p).await {
                addressed = Some(identity);
                break;
            }
        }
        let Some(identity) = addressed else {
            return Ok(());
        };
        let Some(signer) = self.state.keyring.signer(identity.as_deref()).await else {
            return Ok(());
        };
        // NIP-04 ciphertext carries "?iv=" (as nostr-connect tells them apart).
        let nip04 = event.content.contains("?iv=");
        let decrypted = if nip04 { signer.nip04_decrypt(&event.pubkey, &event.content) } else { signer.decrypt(&event.pubkey, &event.content) };
        drop(signer);
        let decrypted = match decrypted {
            Ok(decrypted) => decrypted,
            Err(e) => {
                warn!("Ignoring a NIP-46 event from {} that could not be decrypted: {}", event.pubkey, e);
                return Ok(());
            }
        };
        info!("Received NIP-46 request event: {}", event.id.to_hex());
        let request: Nip46Request = match serde_json::from_str(&decrypted) {
            Ok(request) => request,
            Err(e) => {
                // Not answered, but its sender is still recorded, as the handler records every
                // sender it answers: the encryption methods refuse that key as a peer.
                if let Err(record_error) = self.state.db.record_nip46_sender(&event.pubkey.to_hex(), chrono::Utc::now()).map(|_| ()) {
                    warn!("Failed to record the sender {} of a NIP-46 request: {}", event.pubkey, record_error);
                }
                return Err(e.into());
            }
        };
        let scheme = if nip04 { Scheme::Nip04 } else { Scheme::Nip44 };
        let logout = request.method == "logout";

        let response = match self.state.nip46_handler.handle_for(request, event.pubkey, scheme, identity.clone()).await {
            Nip46Outcome::Reply(response) => response,
            Nip46Outcome::Held { notice: Some(notice), .. } => notice,
            // Held, and the console's address is unknown: the app just waits for the answer.
            Nip46Outcome::Held { notice: None, .. } => return Ok(()),
        };
        let Some(signer) = self.state.keyring.signer(identity.as_deref()).await else {
            return Ok(());
        };
        let sent = self.handle.respond(&signer, event.pubkey, scheme, &response).await;
        drop(signer);
        // A logout ended the connection: once its answer is out, its app's relays may go (#148).
        if logout {
            sync_app_relays(&self.state).await;
        }
        sent
    }
}

/// Makes the relays the bunker listens on for apps connected from their own codes (#148) match the
/// connections still active. Called when one is made or revoked, after a logout, at start, and
/// every few seconds (`main.rs`), which catches every other way a connection ends. A failure is
/// logged: a relay kept too long hears only requests the handler refuses.
pub async fn sync_app_relays(state: &AppState) {
    let Some(handle) = state.relay_handle().await else {
        return;
    };
    let routes = match state.db.nostrconnect_routes(chrono::Utc::now()) {
        Ok(routes) => routes,
        Err(e) => {
            warn!("Failed to read the relays of apps connected from their own codes: {}", e);
            return;
        }
    };
    let routes = routes.into_iter().filter_map(|(client, relays)| PublicKey::from_hex(&client).ok().map(|key| (key, relays))).collect();
    if let Err(e) = handle.set_routes(routes).await {
        warn!("Failed to update the relays of apps connected from their own codes: {}", e);
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
    for reply in replies {
        // Answered as the identity the request was addressed to (#103).
        let Some(signer) = state.keyring.signer(reply.identity.as_deref()).await else {
            warn!("A NIP-46 answer to {} could not be sent: its identity is not unlocked", reply.client_pubkey);
            continue;
        };
        if let Err(e) = handle.respond(&signer, reply.client_pubkey, reply.scheme, &reply.response).await {
            warn!("Failed to send a NIP-46 answer to {}: {}", reply.client_pubkey, e);
        }
    }
}
