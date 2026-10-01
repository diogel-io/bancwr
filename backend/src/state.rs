use std::sync::Arc;
use tokio::sync::RwLock;
use crate::signer::Signer;
use crate::db::Database;
use crate::nip46::Nip46Handler;
use crate::config::Config;
use nostr_relay_pool::RelayPool;

use std::sync::atomic::{AtomicU64, Ordering};

/// Shared application state for both HTTP and NIP-46
#[derive(Clone)]
pub struct AppState {
    pub signer: Arc<RwLock<Signer>>,
    pub db: Arc<Database>,
    pub nip46_handler: Arc<Nip46Handler>,
    pub config: Arc<RwLock<Config>>,
    http_request_count: Arc<AtomicU64>,
    /// The NIP-46 relay pool, once the relay client has started, for the health check (#27).
    relay_pool: Arc<RwLock<Option<RelayPool>>>,
}

impl AppState {
    pub fn new(signer: Signer, db: Database, config: Config) -> Self {
        let signer = Arc::new(RwLock::new(signer));
        let config = Arc::new(RwLock::new(config));
        let db = Arc::new(db);
        let nip46_handler = Arc::new(Nip46Handler::new(signer.clone(), (*db).clone()));
        
        Self {
            signer,
            db,
            nip46_handler,
            config,
            http_request_count: Arc::new(AtomicU64::new(0)),
            relay_pool: Arc::new(RwLock::new(None)),
        }
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
}
