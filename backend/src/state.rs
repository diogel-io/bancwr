use std::sync::Arc;
use tokio::sync::RwLock;
use crate::signer::Signer;
use crate::db::Database;
use crate::keyring::Keyring;
use crate::nip46::Nip46Handler;
use crate::config::Config;
use crate::relay::RelayHandle;
use crate::key_store::{KeyStoreError, RootKey};
use nostr_relay_pool::RelayPool;
use std::path::PathBuf;

use std::sync::atomic::{AtomicU64, Ordering};

/// Shared application state for both HTTP and NIP-46
#[derive(Clone)]
pub struct AppState {
    /// The primary identity's signer: the key in `bunker-key.enc`.
    pub signer: Arc<RwLock<Signer>>,
    /// Every identity this bunker signs as (#103), the primary included.
    pub keyring: Arc<Keyring>,
    pub db: Arc<Database>,
    pub nip46_handler: Arc<Nip46Handler>,
    pub config: Arc<RwLock<Config>>,
    http_request_count: Arc<AtomicU64>,
    /// The NIP-46 relay pool, once the relay client has started, for the health check (#27).
    relay_pool: Arc<RwLock<Option<RelayPool>>>,
    /// The running relay client's handle, once it has started, so a relay list saved in the
    /// console applies at once (#78). None while NIP-46 is off.
    relay_handle: Arc<RwLock<Option<RelayHandle>>>,
    /// Held across a relay list's save and its application, so two saves apply in the order
    /// they were stored (#78).
    pub relays_update: Arc<tokio::sync::Mutex<()>>,
    /// Where the root credential is read from when an identity is created (#146). None:
    /// `BANCWR_ROOT_KEY_FILE`.
    root_key_file: Option<Arc<PathBuf>>,
    /// SPIKE (#151): when set, a member identity needs a bunker session this bunker started after
    /// checking the member's own sign-in (`member_proof`). None: the console's HMAC alone, as today.
    pub member_proof: Option<Arc<crate::member_proof::MemberProof>>,
}

impl AppState {
    pub fn new(signer: Signer, db: Database, config: Config) -> Self {
        Self::with_identities(signer, Vec::new(), db, config)
    }

    /// A bunker signing as `signer`, its primary identity, and `others` (#103).
    pub fn with_identities(signer: Signer, others: Vec<Signer>, db: Database, config: Config) -> Self {
        let signer = Arc::new(RwLock::new(signer));
        let keyring = Arc::new(Keyring::new(signer.clone(), others));
        let nip46_handler = Arc::new(Nip46Handler::with_keyring(keyring.clone(), db.clone()));
        // Requests held for approval (#102): how long they wait, and where the console is.
        nip46_handler.configure_approvals(chrono::Duration::minutes(config.approval_timeout_minutes.into()), config.console_url.clone());
        // What switch_relays answers with (#148): the relays in force, as every reader sees them.
        nip46_handler.configure_relays(config.relay_urls.clone());
        let config = Arc::new(RwLock::new(config));
        let db = Arc::new(db);

        Self {
            signer,
            keyring,
            db,
            nip46_handler,
            config,
            http_request_count: Arc::new(AtomicU64::new(0)),
            relay_pool: Arc::new(RwLock::new(None)),
            relay_handle: Arc::new(RwLock::new(None)),
            relays_update: Arc::new(tokio::sync::Mutex::new(())),
            root_key_file: None,
            member_proof: None,
        }
    }

    /// SPIKE (#151): require the member's own sign-in, checked against `console_url`.
    pub fn with_member_proof(mut self, console_url: &str) -> Self {
        self.member_proof = Some(Arc::new(crate::member_proof::MemberProof::new(console_url)));
        self
    }

    pub fn increment_http_request_count(&self) {
        self.http_request_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn http_request_count(&self) -> u64 {
        self.http_request_count.load(Ordering::SeqCst)
    }

    /// Makes the relay pool visible to the health check. A cheap clone: it shares the pool.
    pub async fn set_relay_pool(&self, pool: RelayPool) {
        *self.relay_pool.write().await = Some(pool);
    }

    pub async fn relay_pool(&self) -> Option<RelayPool> {
        self.relay_pool.read().await.clone()
    }

    pub async fn set_relay_handle(&self, handle: RelayHandle) {
        *self.relay_handle.write().await = Some(handle);
    }

    pub async fn relay_handle(&self) -> Option<RelayHandle> {
        self.relay_handle.read().await.clone()
    }

    /// Reads the root credential when an administrator creates an identity (#146): the file
    /// `BANCWR_ROOT_KEY_FILE` names, or the one `with_root_key_file` gave.
    pub fn root_key(&self) -> Result<RootKey, KeyStoreError> {
        match &self.root_key_file {
            Some(path) => RootKey::from_file(path),
            None => RootKey::from_env(),
        }
    }

    /// Reads the root credential from `path` rather than `BANCWR_ROOT_KEY_FILE`: for tests, which
    /// share one process environment.
    pub fn with_root_key_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.root_key_file = Some(Arc::new(path.into()));
        self
    }

    /// After the keyring changed (#146): the running relay client listens for exactly its
    /// identities from now on. Held under `relays_update`, which the relay client's start holds
    /// too, so an identity created while it starts is either in its first subscription or sent
    /// here. With NIP-46 off there is nothing to change.
    pub async fn identities_changed(&self) -> anyhow::Result<()> {
        let _update = self.relays_update.lock().await;
        match self.relay_handle().await {
            Some(handle) => handle.set_identities(self.keyring.public_keys().await).await,
            None => Ok(()),
        }
    }
}
