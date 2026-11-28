//! The vault: who may use this bunker, and with which role (#24).
//!
//! For 0.1.0 the vault is the `team_members` table, the registry of people's pubkeys allowed to
//! sign in to the administration UI. It is not a keyring: the bunker still signs with its one key.

use std::fmt;
use std::str::FromStr;

use nostr::prelude::*;
use serde::{Deserialize, Serialize};

/// The two roles (#143, which removed #77's third). Their stored and wire form is lowercase; the
/// console shows them as Admin and Signer. Both may hold NIP-46 connections and so sign.
///
/// - `Administrator`: everything, a superset of the signer.
/// - `Signer`: signs through NIP-46 connections and manages their own profile, follows and relays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Administrator,
    Signer,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Administrator => "administrator",
            Role::Signer => "signer",
        }
    }

    /// The name people see (#77).
    pub fn display_name(&self) -> &'static str {
        match self {
            Role::Administrator => "Admin",
            Role::Signer => "Signer",
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

    /// Only the settled names. Every older name (`admin`, #24's `user`, and #77's third role) is
    /// refused, not translated: stored values are migrated once (see `migrations.rs`), and new input
    /// must use the current names (#77, #143).
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "administrator" => Ok(Role::Administrator),
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

/// A grant's role in its department (#147): every grant is a department membership, and a `lead`
/// also manages the department. Stored lowercase in `identity_access.role`. A stored value that is
/// not one of these grants nothing, as an unrecognised team role grants nothing (T4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepartmentRole {
    Member,
    Lead,
}

impl DepartmentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            DepartmentRole::Member => "member",
            DepartmentRole::Lead => "lead",
        }
    }
}

impl fmt::Display for DepartmentRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DepartmentRole {
    type Err = InvalidRole;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "member" => Ok(DepartmentRole::Member),
            "lead" => Ok(DepartmentRole::Lead),
            _ => Err(InvalidRole),
        }
    }
}
