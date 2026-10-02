use nostr::prelude::*;
use nostr::nips::{nip04, nip44};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignerError {
    #[error("Failed to sign event: {0}")]
    SigningError(String),
}

#[derive(Debug, Clone)]
pub struct Signer {
    keys: Keys,
}

impl Signer {
    pub fn new(secret_key: SecretKey) -> Self {
        Self {
            keys: Keys::new(secret_key),
        }
    }

    pub fn update_keys(&mut self, secret_key: SecretKey) {
        self.keys = Keys::new(secret_key);
    }

    pub async fn sign_event(&self, unsigned_event: UnsignedEvent) -> Result<Event, SignerError> {
        let mut unsigned_event = unsigned_event;
        unsigned_event.pubkey = self.keys.public_key();
        
        // Re-calculate ID because we changed the pubkey
        let event_id = EventId::new(
            &unsigned_event.pubkey,
            &unsigned_event.created_at,
            &unsigned_event.kind,
            unsigned_event.tags.as_slice(),
            &unsigned_event.content,
        );
        unsigned_event.id = Some(event_id);

        unsigned_event
            .sign(&self.keys)
            .await
            .map_err(|e| SignerError::SigningError(e.to_string()))
    }

    pub fn decrypt(&self, public_key: &PublicKey, content: &str) -> anyhow::Result<String> {
        let sk = self.keys.secret_key();
        nip44::decrypt(sk, public_key, content).map_err(|e| anyhow::anyhow!("Decryption error: {}", e))
    }

    pub fn encrypt(&self, public_key: &PublicKey, content: &str) -> anyhow::Result<String> {
        let sk = self.keys.secret_key();
        nip44::encrypt(sk, public_key, content, nip44::Version::V2).map_err(|e| anyhow::anyhow!("Encryption error: {}", e))
    }

    /// NIP-04, for NIP-46 clients that still encrypt with it (#52): rust-nostr's 0.39 client does.
    pub fn nip04_decrypt(&self, public_key: &PublicKey, content: &str) -> anyhow::Result<String> {
        nip04::decrypt(self.keys.secret_key(), public_key, content).map_err(|e| anyhow::anyhow!("Decryption error: {}", e))
    }

    pub fn nip04_encrypt(&self, public_key: &PublicKey, content: &str) -> anyhow::Result<String> {
        nip04::encrypt(self.keys.secret_key(), public_key, content).map_err(|e| anyhow::anyhow!("Encryption error: {}", e))
    }

    pub async fn build_event(&self, kind: Kind, content: String, tags: Vec<Tag>) -> anyhow::Result<Event> {
        EventBuilder::new(kind, content)
            .tags(tags)
            .sign(&self.keys)
            .await
            .map_err(|e| anyhow::anyhow!("Event builder error: {}", e))
    }

    /// The bunker's keys, for the relay client to answer NIP-42 AUTH challenges with (#52).
    pub(crate) fn keys(&self) -> Keys {
        self.keys.clone()
    }

    pub fn public_key_bech32(&self) -> String {
        self.keys.public_key().to_bech32().unwrap_or_default()
    }

    /// Lowercase hex, the form the vault stores keys in (#24).
    pub fn public_key_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }
}
