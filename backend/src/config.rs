use std::env;
use std::fs;
use nostr::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("BUNKER_NSEC or BUNKER_NSEC_FILE must be set")]
    MissingNsec,
    #[error("Could not read nsec from file: {0}")]
    FileReadError(#[from] std::io::Error),
    #[error("Invalid nsec: {0}")]
    InvalidNsec(String),
    #[error("BANCWR_ADMIN_PUBKEY is not a valid npub or hex public key")]
    InvalidAdminPubkey,
    #[error("BANCWR_ADMIN_PUBKEY is the bunker's own public key, which can never be registered; set it to the first administrator's key")]
    AdminPubkeyIsBunkerKey,
}

/// The version reported when BANCWR_VERSION is unset or blank, as under `cargo run`. Matches the
/// `ARG BANCWR_VERSION` default in the Dockerfile.
pub const DEFAULT_VERSION: &str = "0.0.0";

/// The running version from a BANCWR_VERSION value. The release workflows set it to the
/// GitVersion semVer (see docs/releasing.md); unset or blank means an unversioned build.
pub fn version_or_default(value: Option<String>) -> String {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_VERSION.to_string())
}

#[derive(Debug, Clone)]
pub struct Config {
    pub secret_key: SecretKey,
    pub port: u16,
    pub db_path: String,
    pub relay_urls: Vec<String>,  // NIP-46 relays to connect to
    pub nip46_enabled: bool,      // Enable NIP-46 protocol
    pub nsec_file: Option<String>,
    pub version: String,          // BANCWR_VERSION, reported by /api/bunker/status (#35)
    /// BANCWR_ADMIN_PUBKEY as canonical hex: the first administrator, seeded when there is none
    /// (#24). Never the bunker's own key.
    pub admin_pubkey: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let db_path = env::var("DATABASE_PATH").unwrap_or_else(|_| "./data/bancwr.db".to_string());

        let mut nsec_file = None;
        let nsec_str = if let Ok(path) = env::var("BUNKER_NSEC_FILE") {
            nsec_file = Some(path.clone());
            fs::read_to_string(path)
                .map(|s| s.trim().to_string())
                .map_err(ConfigError::FileReadError)?
        } else if let Ok(nsec) = env::var("BUNKER_NSEC") {
            nsec
        } else {
            return Err(ConfigError::MissingNsec);
        };

        let secret_key = SecretKey::from_bech32(&nsec_str)
            .or_else(|_| SecretKey::parse(&nsec_str))
            .map_err(|e| ConfigError::InvalidNsec(e.to_string()))?;

        let port = env::var("BUNKER_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);

        let nip46_enabled = env::var("NIP46_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let relay_urls = env::var("NIP46_RELAYS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let version = version_or_default(env::var("BANCWR_VERSION").ok());

        let admin_pubkey = match env::var("BANCWR_ADMIN_PUBKEY") {
            Ok(value) if !value.trim().is_empty() => {
                let hex = crate::registry::canonical_pubkey(&value).map_err(|_| ConfigError::InvalidAdminPubkey)?;
                if hex == Keys::new(secret_key.clone()).public_key().to_hex() {
                    return Err(ConfigError::AdminPubkeyIsBunkerKey);
                }
                Some(hex)
            }
            _ => None,
        };

        Ok(Config {
            secret_key,
            port,
            db_path,
            relay_urls,
            nip46_enabled,
            nsec_file,
            version,
            admin_pubkey,
        })
    }
}
