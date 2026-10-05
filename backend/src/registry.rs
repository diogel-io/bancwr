//! The vault: who may use this bunker, and with which role (#24).
//!
//! For 0.1.0 the vault is the `team_members` table, the registry of people's pubkeys allowed to
//! sign in to the administration UI. It is not a keyring: the bunker still signs with its one key.

use std::fmt;
use std::str::FromStr;

use nostr::prelude::*;
use serde::{Deserialize, Serialize};

/// The three roles (#77, which replaced #24's administrator, user and signer). Their stored and
/// wire form is lowercase; the console shows them as Admin, Signer and Viewer.
///
/// - `Administrator`: everything, a superset of the other two.
/// - `Signer`: signs through NIP-46 connections and manages their own profile, follows and relays.
/// - `Viewer`: reads the team and members' profiles. Never holds a NIP-46 connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Administrator,
    Signer,
    Viewer,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Administrator => "administrator",
            Role::Signer => "signer",
            Role::Viewer => "viewer",
        }
    }

    /// The name people see (#77).
    pub fn display_name(&self) -> &'static str {
        match self {
            Role::Administrator => "Admin",
            Role::Signer => "Signer",
            Role::Viewer => "Viewer",
        }
    }

    /// Whether a member with this role may hold NIP-46 connections and so sign (#77): an
    /// administrator or a signer, never a viewer.
    pub fn can_sign(&self) -> bool {
        matches!(self, Role::Administrator | Role::Signer)
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

    /// Only the settled names. Older names (`admin`, and #24's `user`) are refused, not
    /// translated: stored values are migrated once (see `migrations.rs`), and new input must use
    /// the current names (#77).
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "administrator" => Ok(Role::Administrator),
            "signer" => Ok(Role::Signer),
            "viewer" => Ok(Role::Viewer),
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
