//! The bunker's health, for `GET /api/bunker/status` (#27).
//!
//! Four checks, each reading state the bunker already holds: none opens a connection, so a
//! status request cannot hang on a relay. `/health` stays separate and dependency-free, for the
//! container probe: a flapping relay must not restart the container.
//!
//! Every role can read this (#25's `Access::Health`), so `detail` is fixed wording plus relay URLs:
//! never raw error text, file paths or keys. Raw errors go to the bunker log.

use crate::db::Database;
use crate::signer::Signer;
use crate::state::AppState;
use nostr::prelude::*;
use nostr_relay_pool::RelayStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::warn;

pub const SIGNER: &str = "signer";
pub const DATABASE: &str = "database";
pub const RELAYS: &str = "relays";
pub const ADMINISTRATOR: &str = "administrator";

/// NIP-46 on with no relays from either source (#78).
pub const NO_RELAYS: &str = "NIP-46 is on but no relays are configured. An administrator sets them under Config, Bunker relays, or with NIP46_RELAYS.";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    /// Needs attention, but the bunker still does its job: yellow.
    Warn,
    /// The bunker cannot do its job: red.
    Fail,
    /// Turned off by configuration. Not a fault, and not counted.
    Disabled,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Overall {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RelayHealth {
    pub url: String,
    pub connected: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    /// The relay check only: each configured relay and whether it is connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relays: Option<Vec<RelayHealth>>,
}

impl Check {
    fn new(name: &str, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self { name: name.to_string(), status, detail: detail.into(), relays: None }
    }
}

/// Any failing check is red; otherwise any warning is yellow; otherwise green.
pub fn assess(checks: &[Check]) -> Overall {
    if checks.iter().any(|check| check.status == CheckStatus::Fail) {
        Overall::Unhealthy
    } else if checks.iter().any(|check| check.status == CheckStatus::Warn) {
        Overall::Degraded
    } else {
        Overall::Healthy
    }
}

/// Runs every check and assesses them.
pub async fn report(state: &AppState) -> (Overall, Vec<Check>) {
    let signer = state.signer.read().await.clone();
    let checks = vec![
        check_signer(&signer).await,
        check_database(&state.db),
        check_relays(state).await,
        check_administrator(&state.db),
    ];
    (assess(&checks), checks)
}

/// Signs a throwaway event and verifies it. It is never published. The bunker refuses to start
/// without a key, so "a key is loaded" would always pass; this checks it can still sign.
pub async fn check_signer(signer: &Signer) -> Check {
    let signed = signer
        .build_event(Kind::TextNote, "bancwr health check".to_string(), vec![])
        .await
        .and_then(|event| event.verify().map(|()| event).map_err(anyhow::Error::from));
    match signed {
        Ok(_) => Check::new(SIGNER, CheckStatus::Pass, "The signing key signs and verifies."),
        Err(error) => {
            warn!("Health check: the signer failed: {}", error);
            Check::new(SIGNER, CheckStatus::Fail, "The bunker could not sign. See the bunker log.")
        }
    }
}

/// Reads the audit log's table: a failure here means signatures are not being recorded.
pub fn check_database(db: &Database) -> Check {
    match db.ping() {
        Ok(()) => Check::new(DATABASE, CheckStatus::Pass, "The database answers."),
        Err(error) => {
            warn!("Health check: the database failed: {}", error);
            Check::new(DATABASE, CheckStatus::Fail, "The database did not answer. See the bunker log.")
        }
    }
}

/// Whether anyone can manage the bunker (#74). Without an administrator nobody can register keys,
/// and the only way to get one is BANCWR_ADMIN_PUBKEY at startup; the bunker's own key never can
/// be one (sign-in ADR, rule 10). Yellow rather than red: the bunker still signs for the keys it
/// has. The frontend reads `warn` here as "no administrator" to guide whoever signs in.
pub fn check_administrator(db: &Database) -> Check {
    match db.administrator_count() {
        Ok(0) => Check::new(
            ADMINISTRATOR,
            CheckStatus::Warn,
            "No administrator is registered, so nobody can manage this bunker. Set BANCWR_ADMIN_PUBKEY to the first administrator's npub (never the bunker's own key) and restart the bunker.",
        ),
        Ok(_) => Check::new(ADMINISTRATOR, CheckStatus::Pass, "An administrator is registered."),
        Err(error) => {
            warn!("Health check: could not count administrators: {}", error);
            Check::new(ADMINISTRATOR, CheckStatus::Fail, "Could not read the team. See the bunker log.")
        }
    }
}

/// The relay check, from the configured relays and the ones the pool reports connected.
///
/// NIP-46 is the bunker's only signing path since #25 removed `POST /sign`, so no relay connected
/// is red; some but not all is yellow. NIP-46 turned off is `disabled` and not counted (decided
/// on #27).
pub fn relay_check(enabled: bool, configured: &[String], connected: &HashSet<RelayUrl>) -> Check {
    if !enabled {
        return Check::new(RELAYS, CheckStatus::Disabled, "NIP-46 is turned off (NIP46_ENABLED), so no relays are used.");
    }
    if configured.is_empty() {
        return Check::new(RELAYS, CheckStatus::Fail, NO_RELAYS);
    }

    let relays: Vec<RelayHealth> = configured
        .iter()
        .map(|url| RelayHealth {
            url: url.clone(),
            connected: RelayUrl::parse(url).map(|url| connected.contains(&url)).unwrap_or(false),
        })
        .collect();
    let up = relays.iter().filter(|relay| relay.connected).count();
    let down: Vec<&str> = relays.iter().filter(|relay| !relay.connected).map(|relay| relay.url.as_str()).collect();

    let (status, detail) = if down.is_empty() {
        (CheckStatus::Pass, format!("All {} relays connected.", relays.len()))
    } else if up == 0 {
        (CheckStatus::Fail, format!("No relay connected, so NIP-46 clients cannot reach the bunker. Not connected: {}", down.join(", ")))
    } else {
        (CheckStatus::Warn, format!("{} of {} relays connected. Not connected: {}", up, relays.len(), down.join(", ")))
    };
    Check { relays: Some(relays), ..Check::new(RELAYS, status, detail) }
}

/// The relays the NIP-46 pool reports connected. Connecting or pending counts as not connected:
/// the bunker cannot sign through them yet. Empty before the relay client starts.
pub async fn connected_relays(state: &AppState) -> HashSet<RelayUrl> {
    let mut connected = HashSet::new();
    if let Some(pool) = state.relay_pool().await {
        for (url, relay) in pool.relays().await {
            if relay.status() == RelayStatus::Connected {
                connected.insert(url);
            }
        }
    }
    connected
}

/// The relay check over the relays in force (#78): `NIP46_RELAYS`, or the console's list.
pub async fn check_relays(state: &AppState) -> Check {
    let enabled = state.config.read().await.nip46_enabled;
    let configured = match crate::bunker_relays::effective_relays(state).await {
        Ok(relays) => relays.urls,
        Err(error) => {
            warn!("Health check: could not read the bunker's relays: {}", error);
            return Check::new(RELAYS, CheckStatus::Fail, "Could not read the bunker's relays. See the bunker log.");
        }
    };
    relay_check(enabled, &configured, &connected_relays(state).await)
}
