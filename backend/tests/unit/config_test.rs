use std::env;
use std::sync::Mutex;
use bunker::config::{version_or_default, Config, ConfigError, DEFAULT_VERSION};
use nostr::prelude::*;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_load_needs_no_key() {
    // The signing key is not configuration since #94: it is encrypted at rest and unlocked by
    // key_store. Loading never reads BUNKER_NSEC or BUNKER_NSEC_FILE.
    let _lock = ENV_MUTEX.lock().unwrap();
    env::remove_var("BUNKER_NSEC");
    env::remove_var("BUNKER_NSEC_FILE");
    assert!(Config::load().is_ok());

    env::set_var("BUNKER_NSEC", "not a key");
    assert!(Config::load().is_ok(), "an invalid plain key is not Config's to judge");
    env::remove_var("BUNKER_NSEC");
}

#[test]
fn test_load_port_from_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::set_var("BUNKER_PORT", "4000");

    let config = Config::load().expect("Should load config");
    assert_eq!(config.port, 4000);

    env::remove_var("BUNKER_PORT");
}

#[test]
fn test_default_port() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::remove_var("BUNKER_PORT");

    let config = Config::load().expect("Should load config");
    assert_eq!(config.port, 3000);
}

#[test]
fn test_version_or_default() {
    assert_eq!(version_or_default(Some("0.1.0-49".to_string())), "0.1.0-49");
    assert_eq!(version_or_default(Some(" 0.1.0 \n".to_string())), "0.1.0");
    assert_eq!(version_or_default(None), DEFAULT_VERSION);
    assert_eq!(version_or_default(Some("  ".to_string())), DEFAULT_VERSION);
    assert_eq!(DEFAULT_VERSION, "0.0.0");
}

#[test]
fn test_load_version_from_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::set_var("BANCWR_VERSION", "0.1.0-49");

    let config = Config::load().expect("Should load config from env");
    assert_eq!(config.version, "0.1.0-49");

    env::remove_var("BANCWR_VERSION");
}

#[test]
fn test_default_version_when_unset() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::remove_var("BANCWR_VERSION");

    let config = Config::load().expect("Should load config from env");
    assert_eq!(config.version, "0.0.0");
}

#[test]
fn test_admin_pubkey_is_canonicalised() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let admin = Keys::generate();
    env::set_var("BANCWR_ADMIN_PUBKEY", admin.public_key().to_bech32().unwrap());

    let config = Config::load().expect("Should load config");
    assert_eq!(config.admin_pubkey, Some(admin.public_key().to_hex()));

    env::set_var("BANCWR_ADMIN_PUBKEY", "  ");
    assert_eq!(Config::load().unwrap().admin_pubkey, None, "blank means unset");

    env::remove_var("BANCWR_ADMIN_PUBKEY");
    assert_eq!(Config::load().unwrap().admin_pubkey, None);
}

#[test]
fn test_admin_pubkey_must_be_a_key_and_not_the_bunkers() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let bunker = Keys::generate();

    env::set_var("BANCWR_ADMIN_PUBKEY", "npub1notakey");
    assert!(matches!(Config::load(), Err(ConfigError::InvalidAdminPubkey)));

    // Checked once the key is unlocked, or against the key file's public key while locked.
    env::set_var("BANCWR_ADMIN_PUBKEY", bunker.public_key().to_bech32().unwrap());
    let config = Config::load().expect("a valid key loads");
    assert!(matches!(
        config.check_admin_pubkey(&bunker.public_key().to_hex()),
        Err(ConfigError::AdminPubkeyIsBunkerKey)
    ));
    assert!(config.check_admin_pubkey(&Keys::generate().public_key().to_hex()).is_ok());

    env::remove_var("BANCWR_ADMIN_PUBKEY");
    assert!(Config::load().unwrap().check_admin_pubkey(&bunker.public_key().to_hex()).is_ok(), "unset is fine");
}

#[test]
fn test_proxy_secret_is_required_to_start() {
    let _lock = ENV_MUTEX.lock().unwrap();

    env::remove_var("BANCWR_PROXY_SECRET");
    let config = Config::load().expect("loads without it");
    assert!(matches!(config.require_proxy_secret(), Err(ConfigError::MissingProxySecret)));

    env::set_var("BANCWR_PROXY_SECRET", "short");
    assert!(matches!(Config::load(), Err(ConfigError::ProxySecretTooShort)));

    env::set_var("BANCWR_PROXY_SECRET", "s".repeat(32));
    assert!(Config::load().unwrap().require_proxy_secret().is_ok());

    env::remove_var("BANCWR_PROXY_SECRET");
}
