// Shared by the integration tests: the app on a random port, in either mode, and requests signed
// the way the frontend's Nitro proxy signs them (#25).
#![allow(dead_code)]

use bunker::config::Config;
use bunker::db::Database;
use bunker::proxy_auth::{proxy_signature, Signed, IDENTITY_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
use bunker::registry::Role;
use bunker::server::create_router;
use bunker::signer::Signer;
use bunker::state::AppState;
use nostr::prelude::*;
use reqwest::header::HeaderValue;
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
    /// The app's state, for starting the NIP-46 relay client on it (#53).
    pub state: AppState,
}

/// Enforced: BANCWR_PROXY_SECRET is set, so /api/bunker/* needs a signed identity.
/// Open: it is not, which is how the API behaves until sign-in (#11) requires it.
pub async fn spawn(enforced: bool) -> TestApp {
    spawn_with(enforced, None).await
}

/// As `spawn`, with NIP-46 on and these relays when `nip46_relays` is set (#53). The relay client
/// is not started: tests that need it start `RelayClient` on `app.state`.
pub async fn spawn_with(enforced: bool, nip46_relays: Option<Vec<String>>) -> TestApp {
    spawn_with_identities(enforced, nip46_relays, &[]).await
}

/// As `spawn_with`, holding `others` beside the primary (#103): each recorded as an identity, as
/// `bunker import-key --add` records it, and unlocked, as the bunker unlocks it at start. The
/// administrator is registered before they are added, so they hold the primary's grant.
pub async fn spawn_with_identities(enforced: bool, nip46_relays: Option<Vec<String>>, others: &[(&str, Keys)]) -> TestApp {
    let bunker = Keys::generate();
    let admin = Keys::generate();
    let db = Database::new(":memory:").expect("in-memory database");
    db.add_team_member("Admin", &admin.public_key().to_hex(), Role::Administrator)
        .expect("register the administrator");
    db.ensure_primary_identity(&bunker.public_key().to_hex(), chrono::Utc::now()).expect("record the primary");
    for (label, keys) in others {
        db.add_identity(&keys.public_key().to_hex(), label, "bancwr-key v1 (not used by these tests)", chrono::Utc::now())
            .expect("record the identity")
            .expect("a new identity");
    }

    let config = Config {
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: nip46_relays.clone().unwrap_or_default(),
        nip46_enabled: nip46_relays.is_some(),
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: enforced.then(|| SECRET.to_string()),
        approval_timeout_minutes: bunker::config::DEFAULT_APPROVAL_TIMEOUT_MINUTES,
        console_url: None,
    };
    let others: Vec<Signer> = others.iter().map(|(_, keys)| Signer::new(keys.secret_key().clone())).collect();
    let state = AppState::with_identities(Signer::new(bunker.secret_key().clone()), others, db.clone(), config);
    let app = create_router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind a random port");
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    TestApp { address, client: reqwest::Client::new(), db, bunker, admin, enforced, state }
}

/// A request to the app, signed like the proxy when it carries an identity. The signature is made
/// when the request is built, so it covers the body set with `json` or `body`.
pub struct ProxyRequest {
    client: reqwest::Client,
    builder: RequestBuilder,
    signing: Option<Signing>,
}

struct Signing {
    secret: String,
    timestamp: i64,
    nonce: String,
    path: String,
    identity: String,
}

impl ProxyRequest {
    pub fn json<T: serde::Serialize + ?Sized>(mut self, json: &T) -> Self {
        self.builder = self.builder.json(json);
        self
    }

    pub fn body(mut self, body: impl Into<reqwest::Body>) -> Self {
        self.builder = self.builder.body(body);
        self
    }

    pub fn header(mut self, name: &str, value: impl AsRef<str>) -> Self {
        self.builder = self.builder.header(name, value.as_ref());
        self
    }

    /// Signs with this nonce instead of a fresh one.
    pub fn nonce(mut self, nonce: &str) -> Self {
        if let Some(signing) = self.signing.as_mut() {
            signing.nonce = nonce.to_string();
        }
        self
    }

    /// The finished request, signed over its body. Clone it to send the same request twice.
    pub fn build(self) -> reqwest::Request {
        let mut request = self.builder.build().expect("a valid request");
        if let Some(signing) = self.signing {
            let body = request.body().and_then(|b| b.as_bytes()).unwrap_or_default().to_vec();
            let signature = proxy_signature(
                signing.secret.as_bytes(),
                &Signed {
                    timestamp: signing.timestamp,
                    nonce: &signing.nonce,
                    method: request.method().as_str(),
                    path: &signing.path,
                    identity: &signing.identity,
                    body: &body,
                },
            );
            let headers = request.headers_mut();
            headers.insert(IDENTITY_HEADER, HeaderValue::from_str(&signing.identity).expect("a header value"));
            headers.insert(TIMESTAMP_HEADER, HeaderValue::from(signing.timestamp));
            headers.insert(NONCE_HEADER, HeaderValue::from_str(&signing.nonce).expect("a header value"));
            headers.insert(SIGNATURE_HEADER, HeaderValue::from_str(&signature).expect("a header value"));
        }
        request
    }

    pub async fn send(self) -> reqwest::Result<reqwest::Response> {
        let client = self.client.clone();
        client.execute(self.build()).await
    }
}

/// A fresh nonce in the proxy's format: 32 lowercase hex characters.
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl TestApp {
    /// A request as the administrator: signed when enforced, plain when open.
    pub fn request(&self, method: Method, path: &str) -> ProxyRequest {
        if self.enforced {
            self.signed(method, path, &self.admin.public_key().to_hex())
        } else {
            ProxyRequest { client: self.client.clone(), builder: self.unsigned(method, path), signing: None }
        }
    }

    pub fn get(&self, path: &str) -> ProxyRequest {
        self.request(Method::GET, path)
    }

    /// A request carrying `identity` (a hex pubkey, or `service`), signed like the proxy.
    pub fn signed(&self, method: Method, path: &str, identity: &str) -> ProxyRequest {
        self.signed_with(method, path, identity, SECRET, chrono::Utc::now().timestamp())
    }

    pub fn signed_with(&self, method: Method, path: &str, identity: &str, secret: &str, timestamp: i64) -> ProxyRequest {
        let signing = Signing {
            secret: secret.to_string(),
            timestamp,
            nonce: nonce(),
            path: path.to_string(),
            identity: identity.to_string(),
        };
        ProxyRequest { client: self.client.clone(), builder: self.unsigned(method, path), signing: Some(signing) }
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
