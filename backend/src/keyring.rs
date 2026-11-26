//! The identities this bunker signs as (#103), unlocked.
//!
//! The primary identity is the key in `bunker-key.enc`, the one `AppState::signer` has always
//! held. Each further identity was added with `bunker import-key --add`, encrypted under the same
//! root credential, and is unlocked at start beside it. Each is its own NIP-46 remote signer: a
//! request is answered by the identity its `p` tag names.
//!
//! Tokens, connections, grants and the signing log refer to an identity by its hex pubkey, and to
//! the primary by None, so every row written before #103 means the primary.
//!
//! The identities beside the primary change while the bunker runs (#146): an administrator creates
//! one in the console, or retires one. They sit behind a plain lock that is only ever held for a
//! map lookup or change, never across an await, so a request never waits on another's relay I/O.
use crate::signer::Signer;
use nostr::prelude::*;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::{Arc, RwLock as SyncRwLock};
use tokio::sync::{RwLock, RwLockReadGuard};

/// The unlocked identities: the primary, and the others by hex pubkey.
pub struct Keyring {
    primary: Arc<RwLock<Signer>>,
    others: SyncRwLock<BTreeMap<String, Arc<Signer>>>,
}

/// One identity's signer, borrowed for a request.
pub enum SignerRef<'a> {
    Primary(RwLockReadGuard<'a, Signer>),
    Other(Arc<Signer>),
}

impl Deref for SignerRef<'_> {
    type Target = Signer;

    fn deref(&self) -> &Signer {
        match self {
            Self::Primary(guard) => guard,
            Self::Other(signer) => signer,
        }
    }
}

impl Keyring {
    /// The primary, and `others` (none for a single-key bunker).
    pub fn new(primary: Arc<RwLock<Signer>>, others: Vec<Signer>) -> Self {
        let others = others.into_iter().map(|signer| (signer.public_key_hex(), Arc::new(signer))).collect();
        Self { primary, others: SyncRwLock::new(others) }
    }

    fn others_map(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, Arc<Signer>>> {
        // A poisoned lock still holds a whole map: every change is one insert or remove.
        self.others.read().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Unlocks another identity while the bunker runs (#146). False when it is already one, the
    /// primary included, and then nothing changes.
    pub async fn add(&self, signer: Signer) -> bool {
        let hex = signer.public_key_hex();
        if self.primary.read().await.public_key_hex() == hex {
            return false;
        }
        let mut others = self.others.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        if others.contains_key(&hex) {
            return false;
        }
        others.insert(hex, Arc::new(signer));
        true
    }

    /// Drops an identity other than the primary (#146): from now on a request addressed to it is
    /// ignored, as for any key the bunker does not hold. False when it was not unlocked.
    pub fn remove(&self, pubkey: &str) -> bool {
        self.others.write().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(pubkey).is_some()
    }

    /// The signer for `identity` (None: the primary), or None when it is not one of this bunker's.
    pub async fn signer(&self, identity: Option<&str>) -> Option<SignerRef<'_>> {
        match identity {
            None => Some(SignerRef::Primary(self.primary.read().await)),
            Some(pubkey) => self.others_map().get(pubkey).cloned().map(SignerRef::Other),
        }
    }

    /// How a request addressed to `pubkey` refers to its identity: `Some(None)` for the primary,
    /// `Some(Some(hex))` for another, and None when `pubkey` is not one of this bunker's.
    pub async fn identity_of(&self, pubkey: &PublicKey) -> Option<Option<String>> {
        let hex = pubkey.to_hex();
        if self.primary.read().await.public_key_hex() == hex {
            return Some(None);
        }
        self.others_map().contains_key(&hex).then_some(Some(hex))
    }

    /// Every identity's public key, the primary first.
    pub async fn public_keys(&self) -> Vec<PublicKey> {
        let primary = self.primary.read().await.public_key();
        let others: Vec<PublicKey> = self.others_map().values().map(|signer| signer.public_key()).collect();
        std::iter::once(primary).chain(others).collect()
    }

    /// Whether `pubkey` (hex) is any of this bunker's identities.
    pub async fn is_identity(&self, pubkey: &str) -> bool {
        self.primary.read().await.public_key_hex() == pubkey || self.others_map().contains_key(pubkey)
    }

    /// How many identities beside the primary are unlocked.
    pub fn others(&self) -> usize {
        self.others_map().len()
    }
}
