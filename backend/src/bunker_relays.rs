//! The bunker's own NIP-46 relays (#78): which list is in force, and what a list may hold.
//!
//! Two sources, and the rule between them lives here and nowhere else:
//!
//! - `NIP46_RELAYS`, when set and non-empty, always decides (source `environment`). The console
//!   shows it read-only and refuses to change it, so an operator who pins relays in compose is
//!   never silently overridden. A list stored in the console is then ignored, not merged.
//! - Otherwise the list an administrator saved in the console (source `console`), kept in the
//!   local database and never published to Nostr. There is no seeding from the variable.
//!
//! Every reader of the bunker's relays (the relay client at startup, token `bunker://` strings,
//! the health check, `GET /api/bunker/relays`) goes through `effective`.
//!
//! These are not members' NIP-65 relay lists (`/relays` in the console, #33): the bunker reaches
//! NIP-46 clients through these, as itself.

use crate::db::Database;
use crate::state::AppState;
use nostr::prelude::*;
use serde::{Deserialize, Serialize};

/// At most this many relays: each is a connection the bunker holds open, and each is repeated in
/// every `bunker://` string.
pub const MAX_RELAYS: usize = 6;
/// The console warns below this: one relay is a single point of failure for every connected app.
pub const ADVISED_MIN_RELAYS: usize = 2;
/// As the console's own limit (frontend `MAX_RELAY_URL_LENGTH`).
pub const MAX_RELAY_URL_LENGTH: usize = 255;

/// Hosts that may be reached over unencrypted `ws://`: this machine only. Anything else must be
/// `wss://`, so NIP-46 traffic (already encrypted end to end) also hides who talks to the bunker.
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RelaySource {
    /// `NIP46_RELAYS`: read-only in the console.
    Environment,
    /// Saved by an administrator in the console.
    Console,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRelays {
    pub source: RelaySource,
    pub urls: Vec<String>,
}

/// The relays in force: `env_relays` (the parsed `NIP46_RELAYS`) when there are any, otherwise
/// the stored list.
pub fn effective(env_relays: &[String], db: &Database) -> anyhow::Result<EffectiveRelays> {
    if !env_relays.is_empty() {
        return Ok(EffectiveRelays { source: RelaySource::Environment, urls: env_relays.to_vec() });
    }
    let urls = db.list_bunker_relays()?.into_iter().map(|relay| relay.url).collect();
    Ok(EffectiveRelays { source: RelaySource::Console, urls })
}

/// `effective`, from the running bunker's configuration and database.
pub async fn effective_relays(state: &AppState) -> anyhow::Result<EffectiveRelays> {
    let env_relays = state.config.read().await.relay_urls.clone();
    effective(&env_relays, &state.db)
}

/// Why a relay list was refused. Each maps to a 400 with its own `error` code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Not a relay address the bunker will use; `reason` says why, for the administrator.
    InvalidUrl { url: String, reason: &'static str },
    /// The same relay twice, after normalising.
    Duplicate { url: String },
    TooMany,
    /// No relays while NIP-46 is on: no app could reach the bunker.
    Empty,
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::InvalidUrl { .. } => "invalid_relay_url",
            Refusal::Duplicate { .. } => "duplicate_relay",
            Refusal::TooMany => "too_many_relays",
            Refusal::Empty => "no_relays",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Refusal::InvalidUrl { url, reason } => format!("{} is not a relay the bunker can use: {}", url, reason),
            Refusal::Duplicate { url } => format!("{} is listed twice.", url),
            Refusal::TooMany => format!("The bunker uses at most {} relays.", MAX_RELAYS),
            Refusal::Empty => "NIP-46 is on, so the bunker needs at least one relay: without one, no app can reach it.".to_string(),
        }
    }
}

/// One relay address, normalised as the console normalises it (`normalizeRelayUrl`): trimmed,
/// scheme and host lowercased, and the trailing slash of an empty path dropped. `wss://`, or
/// `ws://` on this machine only; no credentials and no fragment.
pub fn normalise_relay_url(input: &str) -> Result<String, &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("the address is empty.");
    }
    let url = Url::parse(trimmed).map_err(|_| "it is not a valid address, such as wss://relay.example.com.")?;
    let host = url.host_str().filter(|host| !host.is_empty()).ok_or("the address needs a hostname.")?;
    match url.scheme() {
        "wss" => {}
        "ws" if LOOPBACK_HOSTS.contains(&host) => {}
        "ws" => return Err("only a relay on this machine (localhost) may use unencrypted ws://; use wss://."),
        _ => return Err("relay addresses start with wss://."),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a relay address cannot carry a user name or password.");
    }
    if url.fragment().is_some() {
        return Err("a relay address cannot contain a #fragment.");
    }
    let mut normalised = url.as_str().to_string();
    if url.path() == "/" && url.query().is_none() && normalised.ends_with('/') {
        normalised.pop();
    }
    if normalised.len() > MAX_RELAY_URL_LENGTH {
        return Err("relay addresses can be up to 255 characters.");
    }
    // What the relay client will be handed must parse there too.
    RelayUrl::parse(&normalised).map_err(|_| "it is not a valid relay address.")?;
    Ok(normalised)
}

/// Checks a whole list for `PUT /api/bunker/relays` and returns it normalised, in order. The
/// first problem found is returned, address problems before the count, so the administrator
/// fixes what they typed first.
pub fn validate(input: &[String], nip46_enabled: bool) -> Result<Vec<String>, Refusal> {
    let mut urls: Vec<String> = Vec::with_capacity(input.len());
    for raw in input {
        let url = normalise_relay_url(raw).map_err(|reason| Refusal::InvalidUrl { url: raw.trim().to_string(), reason })?;
        if urls.contains(&url) {
            return Err(Refusal::Duplicate { url });
        }
        urls.push(url);
    }
    if urls.len() > MAX_RELAYS {
        return Err(Refusal::TooMany);
    }
    if urls.is_empty() && nip46_enabled {
        return Err(Refusal::Empty);
    }
    Ok(urls)
}
