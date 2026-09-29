//! The vault: who may use this bunker, and with which role (#24).
//!
//! For 0.1.0 the vault is the `team_members` table, the registry of people's pubkeys allowed to
//! sign in to the administration UI. It is not a keyring: the bunker still signs with its one key.

use std::fmt;
use std::str::FromStr;

use nostr::prelude::*;
use serde::{Deserialize, Serialize};

/// The three roles from the 0.1.0 features doc. Their stored and wire form is lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Administrator,
    User,
    Signer,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Administrator => "administrator",
            Role::User => "user",
            Role::Signer => "signer",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidRole;

impl FromStr for Role {
    type Err = InvalidRole;

    /// Only the settled names. The old `admin` and `viewer` are refused, not translated: stored
    /// values are migrated once (see `db.rs`), and new input must use the new names.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "administrator" => Ok(Role::Administrator),
            "user" => Ok(Role::User),
            "signer" => Ok(Role::Signer),
            _ => Err(InvalidRole),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidPubkey;

/// The one stored form of a pubkey: lowercase hex. Accepts an `npub`, hex (any case) or a
/// `nostr:` URI, and rejects anything that is not a valid key, so one key cannot be registered
/// twice under two encodings.
pub fn canonical_pubkey(input: &str) -> Result<String, InvalidPubkey> {
    PublicKey::parse(input.trim())
        .map(|key| key.to_hex())
        .map_err(|_| InvalidPubkey)
}

/// The `npub` of a stored pubkey, for display. `None` for a value that is not a valid key, which
/// only rows stored before #24 can hold.
pub fn npub(stored: &str) -> Option<String> {
    PublicKey::from_hex(stored).ok().and_then(|key| key.to_bech32().ok())
}
