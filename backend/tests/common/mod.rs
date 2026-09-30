// Shared by the integration tests: the app on a random port, in either mode, and requests signed
// the way the frontend's Nitro proxy signs them (#25).
#![allow(dead_code)]

use bunker::config::Config;
use bunker::db::Database;
use bunker::proxy_auth::{proxy_signature, IDENTITY_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
use bunker::registry::Role;
use bunker::server::create_router;
use bunker::signer::Signer;
use bunker::state::AppState;
use nostr::prelude::*;
use reqwest::{Method, RequestBuilder};
use tokio::net::TcpListener;

pub const SECRET: &str = "bancwr-test-proxy-secret-0123456789abcdef";

pub struct TestApp {
    pub address: String,
    pub client: reqwest::Client,
    /// The same database the app uses, to arrange state directly.
    pub db: Database,
    /// The bunker's own key.
    pub bunker: Keys,
    /// A registered administrator. Enforced requests from `get`/`request` are signed as them.
    pub admin: Keys,
    pub enforced: bool,
}

/// Enforced: BANCWR_PROXY_SECRET is set, so /api/bunker/* needs a signed identity.
/// Open: it is not, which is how the API behaves until sign-in (#11) requires it.
pub async fn spawn(enforced: bool) -> TestApp {
    let bunker = Keys::generate();
    let admin = Keys::generate();
    let db = Database::new(":memory:").expect("in-memory database");
    db.add_team_member("Admin", &admin.public_key().to_hex(), Role::Administrator)
        .expect("register the administrator");

    let config = Config {
        secret_key: bunker.secret_key().clone(),
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: enforced.then(|| SECRET.to_string()),
    };
    let state = AppState::new(Signer::new(bunker.secret_key().clone()), db.clone(), config);
    let app = create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind a random port");
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    TestApp { address, client: reqwest::Client::new(), db, bunker, admin, enforced }
}

impl TestApp {
    /// A request as the administrator: signed when enforced, plain when open.
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        if self.enforced {
            self.signed(method, path, &self.admin.public_key().to_hex())
        } else {
            self.unsigned(method, path)
        }
    }

    pub fn get(&self, path: &str) -> RequestBuilder {
        self.request(Method::GET, path)
    }

    /// A request carrying `identity` (a hex pubkey, or `service`), signed like the proxy.
    pub fn signed(&self, method: Method, path: &str, identity: &str) -> RequestBuilder {
        self.signed_with(method, path, identity, SECRET, chrono::Utc::now().timestamp())
    }

    pub fn signed_with(&self, method: Method, path: &str, identity: &str, secret: &str, timestamp: i64) -> RequestBuilder {
        let signature = proxy_signature(secret.as_bytes(), timestamp, method.as_str(), path, identity);
        self.unsigned(method, path)
            .header(IDENTITY_HEADER, identity)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(SIGNATURE_HEADER, signature)
    }

    pub fn unsigned(&self, method: Method, path: &str) -> RequestBuilder {
        self.client.request(method, format!("{}{}", self.address, path))
    }

    /// Registers a key with a role and returns its hex pubkey.
    pub fn register(&self, role: Role) -> Keys {
        let keys = Keys::generate();
        self.db.add_team_member("Member", &keys.public_key().to_hex(), role).expect("register");
        keys
    }
}
