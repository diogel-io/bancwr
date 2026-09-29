use std::env;
use std::fs;
use std::sync::Mutex;
use bunker::config::{version_or_default, Config, DEFAULT_VERSION};
use nostr::prelude::*;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_load_nsec_from_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    let test_nsec = keys.secret_key().to_bech32().unwrap();
    env::set_var("BUNKER_NSEC", &test_nsec);
    env::remove_var("BUNKER_NSEC_FILE");

    let config = Config::load().expect("Should load config from env");
    assert_eq!(config.secret_key.to_bech32().unwrap(), test_nsec);

    env::remove_var("BUNKER_NSEC");
}

#[test]
fn test_load_port_from_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    let test_nsec = keys.secret_key().to_bech32().unwrap();
    env::set_var("BUNKER_NSEC", &test_nsec);
    env::set_var("BUNKER_PORT", "4000");

    let config = Config::load().expect("Should load config");
    assert_eq!(config.port, 4000);

    env::remove_var("BUNKER_NSEC");
    env::remove_var("BUNKER_PORT");
}

#[test]
fn test_default_port() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    let test_nsec = keys.secret_key().to_bech32().unwrap();
    env::set_var("BUNKER_NSEC", &test_nsec);
    env::remove_var("BUNKER_PORT");

    let config = Config::load().expect("Should load config");
    assert_eq!(config.port, 3000);

    env::remove_var("BUNKER_NSEC");
}

#[test]
fn test_load_nsec_from_file() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    let test_nsec = keys.secret_key().to_bech32().unwrap();
    let file_path = "test_nsec_file";
    fs::write(file_path, &test_nsec).unwrap();

    env::remove_var("BUNKER_NSEC");
    env::set_var("BUNKER_NSEC_FILE", file_path);

    let config = Config::load().expect("Should load config from file");
    assert_eq!(config.secret_key.to_bech32().unwrap(), test_nsec);

    env::remove_var("BUNKER_NSEC_FILE");
    fs::remove_file(file_path).unwrap();
}

#[test]
fn test_load_nsec_file_path() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    let test_nsec = keys.secret_key().to_bech32().unwrap();
    let file_path = "test_nsec_file_path";
    fs::write(file_path, &test_nsec).unwrap();

    env::remove_var("BUNKER_NSEC");
    env::set_var("BUNKER_NSEC_FILE", file_path);

    let config = Config::load().expect("Should load config from file");
    assert_eq!(config.nsec_file, Some(file_path.to_string()));

    env::remove_var("BUNKER_NSEC_FILE");
    fs::remove_file(file_path).unwrap();
}

#[test]
fn test_load_nsec_fails_on_missing_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::remove_var("BUNKER_NSEC");
    env::remove_var("BUNKER_NSEC_FILE");

    let result = Config::load();
    assert!(result.is_err());
}

#[test]
fn test_load_nsec_fails_on_invalid_nsec() {
    let _lock = ENV_MUTEX.lock().unwrap();
    env::set_var("BUNKER_NSEC", "invalid_nsec");
    env::remove_var("BUNKER_NSEC_FILE");

    let result = Config::load();
    assert!(result.is_err());

    env::remove_var("BUNKER_NSEC");
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
    let keys = Keys::generate();
    env::set_var("BUNKER_NSEC", keys.secret_key().to_bech32().unwrap());
    env::remove_var("BUNKER_NSEC_FILE");
    env::set_var("BANCWR_VERSION", "0.1.0-49");

    let config = Config::load().expect("Should load config from env");
    assert_eq!(config.version, "0.1.0-49");

    env::remove_var("BANCWR_VERSION");
    env::remove_var("BUNKER_NSEC");
}

#[test]
fn test_default_version_when_unset() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let keys = Keys::generate();
    env::set_var("BUNKER_NSEC", keys.secret_key().to_bech32().unwrap());
    env::remove_var("BUNKER_NSEC_FILE");
    env::remove_var("BANCWR_VERSION");

    let config = Config::load().expect("Should load config from env");
    assert_eq!(config.version, "0.0.0");

    env::remove_var("BUNKER_NSEC");
}
