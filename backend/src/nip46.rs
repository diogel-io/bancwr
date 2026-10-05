//! NIP-46 remote signing (#53), read at nips commit 0046368a.
//!
//! A client may use the bunker only after `connect` with the secret of a single-use connection
//! token an administrator issued for a vault member. The connection is stored (it survives a
//! restart), attributed to that member, and allowed exactly the permissions granted: the token's,
//! intersected with what the client asked for. Every request after `connect` needs an active
//! connection whose member is still in the vault and may sign: an administrator or a signer, never
//! a viewer (#77), whose connections are refused as a removed member's are. Clients sign as the
//! bunker's key: "for a member" is attribution and accountability, not a separate key.
use crate::db::{ClientMetadata, Database, Nip46Connection, RedeemRefusal};
use crate::signer::Signer;
use chrono::Utc;
use nostr::hashes::{sha256, Hash};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// NIP-46 JSON-RPC request
#[derive(Debug, Deserialize, Serialize)]
pub struct Nip46Request {
    pub id: String,
    pub method: String,
    pub params: Vec<String>,
}

/// NIP-46 JSON-RPC response
#[derive(Debug, Serialize, Deserialize)]
pub struct Nip46Response {
    pub id: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl Nip46Response {
    fn ok(id: String, result: impl Into<String>) -> Self {
        Self { id, result: Some(result.into()), error: None }
    }

    fn err(id: String, error: impl Into<String>) -> Self {
        Self { id, result: None, error: Some(error.into()) }
    }
}

/// The hex SHA-256 of a connection secret, which is all the database keeps of it.
pub fn hash_secret(secret: &str) -> String {
    sha256::Hash::hash(secret.as_bytes()).to_string()
}

/// The permission a `sign_event` of `kind` needs.
pub fn sign_permission(kind: u16) -> String {
    format!("sign_event:{}", kind)
}

/// Shown to a client that has not connected, or whose connection ended.
pub const NOT_CONNECTED: &str = "Not connected: connect with a connection token first";

/// NIP-46 protocol handler
pub struct Nip46Handler {
    signer: Arc<RwLock<Signer>>,
    db: Database,
}

impl Nip46Handler {
    pub fn new(signer: Arc<RwLock<Signer>>, db: Database) -> Self {
        Self { signer, db }
    }

    /// Active connections, for /api/bunker/metrics.
    pub async fn connection_count(&self) -> usize {
        self.db.active_nip46_connection_count().unwrap_or_else(|e| {
            warn!("Failed to count NIP-46 connections: {}", e);
            0
        })
    }

    /// Handle incoming NIP-46 request
    pub async fn handle_request(&self, request: Nip46Request, client_pubkey: PublicKey) -> Nip46Response {
        info!("Handling NIP-46 request: method={}, id={}", request.method, request.id);
        if request.method == "connect" {
            return self.handle_connect(request, client_pubkey).await;
        }

        let connection = match self.db.active_nip46_connection(&client_pubkey.to_hex()) {
            Ok(Some(connection)) => connection,
            Ok(None) => return Nip46Response::err(request.id, NOT_CONNECTED),
            Err(e) => {
                warn!("Failed to read the NIP-46 connection of {}: {}", client_pubkey, e);
                return Nip46Response::err(request.id, "Internal error");
            }
        };

        match request.method.as_str() {
            "sign_event" => self.handle_sign_event(request, client_pubkey, &connection).await,
            "get_public_key" => Nip46Response::ok(request.id, self.signer.read().await.public_key_hex()),
            "ping" => Nip46Response::ok(request.id, "pong"),
            // No relay change is proposed. An administrator can change the bunker's relays (#78),
            // but moving connected apps to the new list is out of its scope: an app on a removed
            // relay reconnects with a new bunker:// string.
            "switch_relays" => Nip46Response::ok(request.id, "null"),
            "logout" => {
                // NIP-46: reply "ack", and remove the session. The reply is built first; the
                // revocation cannot fail in a way the client could act on.
                if let Err(e) = self.db.revoke_nip46_connection(&connection.id, "logout", Utc::now()) {
                    warn!("Failed to revoke the NIP-46 connection {} on logout: {}", connection.id, e);
                }
                Nip46Response::ok(request.id, "ack")
            }
            "nip04_encrypt" | "nip04_decrypt" | "nip44_encrypt" | "nip44_decrypt" => {
                Nip46Response::err(request.id, format!("Not supported: {}", request.method))
            }
            _ => Nip46Response::err(request.id, format!("Unknown method: {}", request.method)),
        }
    }

    /// `connect`: `[remote-signer-pubkey, secret, requested-perms?, client-metadata?]`.
    async fn handle_connect(&self, request: Nip46Request, client_pubkey: PublicKey) -> Nip46Response {
        let bunker = self.signer.read().await.public_key_hex();
        if request.params.first().map(|p| p.trim().to_lowercase()) != Some(bunker) {
            return Nip46Response::err(request.id, "This connect is not addressed to this bunker");
        }
        let secret = request.params.get(1).map(|s| s.trim()).unwrap_or("");
        if secret.is_empty() {
            return Nip46Response::err(request.id, "A connection token's secret is required");
        }
        let requested = request.params.get(2).map(String::as_str);
        let metadata = request.params.get(3).map(|m| parse_metadata(m)).unwrap_or_default();

        match self.db.redeem_nip46_token(&hash_secret(secret), &client_pubkey.to_hex(), requested, &metadata, Utc::now()) {
            Ok(Ok(connection)) => {
                info!("NIP-46 client {} connected for {} with {}", client_pubkey, connection.for_pubkey, connection.perms.join(","));
                Nip46Response::ok(request.id, "ack")
            }
            Ok(Err(refusal)) => {
                warn!("NIP-46 connect from {} refused: {:?}", client_pubkey, refusal);
                Nip46Response::err(request.id, match refusal {
                    RedeemRefusal::UnknownSecret => "Invalid secret",
                    RedeemRefusal::Used => "This connection token has already been used",
                    RedeemRefusal::Revoked => "This connection token has been revoked",
                    RedeemRefusal::Expired => "This connection token has expired",
                    RedeemRefusal::MemberRemoved => "This connection token's member is no longer in the vault",
                    RedeemRefusal::MemberCannotSign => "This connection token's member is a viewer, who cannot sign",
                    RedeemRefusal::NothingGrantable => "None of the requested permissions are allowed by this connection token",
                })
            }
            Err(e) => {
                warn!("Failed to redeem a NIP-46 connection token from {}: {}", client_pubkey, e);
                Nip46Response::err(request.id, "Internal error")
            }
        }
    }

    /// sign_event: only kinds the connection was granted.
    async fn handle_sign_event(&self, request: Nip46Request, client_pubkey: PublicKey, connection: &Nip46Connection) -> Nip46Response {
        let Some(unsigned_event_json) = request.params.first() else {
            return Nip46Response::err(request.id, "Missing event to sign");
        };
        // NIP-46 defines the parameter as `{kind, content, tags, created_at}`, with no pubkey, and
        // nostr-tools sends exactly that; some clients add a pubkey. Either way the event is signed
        // as the bunker's key, so the bunker's key is what goes in (#31 found the strict parse).
        let mut template: serde_json::Value = match serde_json::from_str(unsigned_event_json) {
            Ok(value @ serde_json::Value::Object(_)) => value,
            Ok(_) => return Nip46Response::err(request.id, "Invalid event JSON: expected an object"),
            Err(e) => return Nip46Response::err(request.id, format!("Invalid event JSON: {}", e)),
        };
        template["pubkey"] = serde_json::Value::String(self.signer.read().await.public_key_hex());
        if let Some(object) = template.as_object_mut() {
            // Any id the client computed was for its own idea of the event; it is recomputed.
            object.remove("id");
        }
        let unsigned_event: UnsignedEvent = match serde_json::from_value(template) {
            Ok(ev) => ev,
            Err(e) => return Nip46Response::err(request.id, format!("Invalid event JSON: {}", e)),
        };
        let kind = unsigned_event.kind.as_u16();
        if !connection.perms.contains(&sign_permission(kind)) {
            return Nip46Response::err(request.id, format!("Forbidden: this connection may not sign kind {}", kind));
        }

        match self.signer.read().await.sign_event(unsigned_event).await {
            Ok(signed_event) => {
                let now = Utc::now();
                if let Err(e) = self.db.touch_nip46_connection(&connection.id, now) {
                    warn!("Failed to record NIP-46 connection use: {}", e);
                }
                // The client asked; the member is who the connection signs for (diogel-io/workspace#38).
                if let Err(e) = self.db.log_signing_event(
                    &signed_event.id.to_hex(),
                    &client_pubkey.to_hex(),
                    Some(&connection.for_pubkey),
                    Some(&connection.id),
                    kind as u32,
                    now,
                ) {
                    warn!("Failed to log signing event: {}", e);
                }
                Nip46Response::ok(request.id, signed_event.as_json())
            }
            Err(e) => Nip46Response::err(request.id, format!("Signing error: {}", e)),
        }
    }
}

/// NIP-46 client metadata: a JSON object with optional `name`, `url` and `image`. Self-reported
/// and unauthenticated, so kept only as display hints, trimmed and bounded; anything malformed is
/// dropped rather than refusing the connection.
fn parse_metadata(raw: &str) -> ClientMetadata {
    let value: serde_json::Value = serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
    let field = |key: &str, max: usize| {
        value.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(max).collect::<String>())
    };
    let link = |key: &str| field(key, 512).filter(|v| v.starts_with("https://") || v.starts_with("http://"));
    ClientMetadata { name: field("name", 100), url: link("url"), image: link("image") }
}
