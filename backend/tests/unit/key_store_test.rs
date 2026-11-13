// The bunker key at rest (#94): encrypt, decrypt, a wrong root credential, tampered files, and how
// startup decides between unlocked and locked.
use bunker::key_store::{
    self, decrypt, default_key_path, encrypt, import, recorded_public_key, unlock, write_new, KeyStoreError, RootKey, Unlock,
    PLAIN_KEY_NOT_IMPORTED, PLAIN_KEY_STILL_SET,
};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use nostr::prelude::*;
use nostr::util::hex;
use std::path::Path;

const ROOT_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const OTHER_HEX: &str = "ff0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn root() -> RootKey {
    RootKey::from_hex(ROOT_HEX).unwrap()
}

fn fields(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn join(fields: &[String]) -> String {
    format!("{}\n", fields.join(" "))
}

/// Flips one bit in a hex field.
fn flip(hex_field: &str, byte: usize) -> String {
    let mut bytes = hex::decode(hex_field).unwrap();
    bytes[byte] ^= 0x01;
    hex::encode(bytes)
}

#[test]
fn encrypts_and_decrypts() {
    let keys = Keys::generate();
    let text = encrypt(&root(), keys.secret_key());
    let decrypted = decrypt(&root(), &text).unwrap();
    assert_eq!(decrypted.to_secret_hex(), keys.secret_key().to_secret_hex());
    assert_eq!(recorded_public_key(&text).unwrap(), keys.public_key());
}

#[test]
fn each_encryption_uses_a_fresh_nonce() {
    let keys = Keys::generate();
    assert_ne!(encrypt(&root(), keys.secret_key()), encrypt(&root(), keys.secret_key()));
}

#[test]
fn the_file_holds_no_form_of_the_key() {
    let keys = Keys::generate();
    let text = encrypt(&root(), keys.secret_key());
    assert!(!text.contains(&keys.secret_key().to_bech32().unwrap()));
    assert!(!text.contains(&keys.secret_key().to_secret_hex()));
    let raw = keys.secret_key().to_secret_bytes();
    assert!(!text.as_bytes().windows(raw.len()).any(|window| window == raw));
    assert!(!text.contains(ROOT_HEX), "nor the root credential");
}

#[test]
fn a_wrong_root_credential_does_not_decrypt() {
    let text = encrypt(&root(), Keys::generate().secret_key());
    let other = RootKey::from_hex(OTHER_HEX).unwrap();
    assert!(matches!(decrypt(&other, &text), Err(KeyStoreError::WrongRootKeyOrTampered)));
}

#[test]
fn tampered_ciphertext_nonce_or_public_key_does_not_decrypt() {
    let text = encrypt(&root(), Keys::generate().secret_key());
    let original = fields(&text);

    for (field, byte) in [(4, 0), (4, 31), (4, 47), (3, 0), (3, 23)] {
        let mut changed = original.clone();
        changed[field] = flip(&changed[field], byte);
        assert!(
            matches!(decrypt(&root(), &join(&changed)), Err(KeyStoreError::WrongRootKeyOrTampered)),
            "field {} byte {}",
            field,
            byte
        );
    }

    // Another valid public key in the header: authenticated as associated data.
    let mut changed = original.clone();
    changed[2] = Keys::generate().public_key().to_hex();
    assert!(matches!(decrypt(&root(), &join(&changed)), Err(KeyStoreError::WrongRootKeyOrTampered)));

    // The same public key in upper case: the header is authenticated as written.
    let mut changed = original.clone();
    changed[2] = changed[2].to_uppercase();
    assert!(decrypt(&root(), &join(&changed)).is_err());
}

#[test]
fn a_changed_magic_version_or_truncated_file_is_malformed() {
    let text = encrypt(&root(), Keys::generate().secret_key());
    let original = fields(&text);

    for (field, value) in [(0, "bancwr-kez"), (1, "v2")] {
        let mut changed = original.clone();
        changed[field] = value.to_string();
        assert!(matches!(decrypt(&root(), &join(&changed)), Err(KeyStoreError::KeyFileMalformed)));
    }
    assert!(matches!(decrypt(&root(), &join(&original[..4])), Err(KeyStoreError::KeyFileMalformed)));
    assert!(matches!(decrypt(&root(), &text[..text.len() / 2]), Err(KeyStoreError::KeyFileMalformed)));
    assert!(matches!(decrypt(&root(), ""), Err(KeyStoreError::KeyFileMalformed)));

    let mut short_nonce = original.clone();
    short_nonce[3].truncate(46);
    assert!(matches!(decrypt(&root(), &join(&short_nonce)), Err(KeyStoreError::KeyFileMalformed)));
}

#[test]
fn a_key_that_does_not_match_the_recorded_public_key_is_refused() {
    // A file someone built with the root credential, recording another key's public key.
    let keys = Keys::generate();
    let claimed = Keys::generate().public_key();
    let header = format!("bancwr-key v1 {}", claimed.to_hex());
    let nonce = [7u8; 24];
    let cipher = XChaCha20Poly1305::new(hex::decode(ROOT_HEX).unwrap().as_slice().into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: &keys.secret_key().to_secret_bytes(), aad: header.as_bytes() })
        .unwrap();
    let text = format!("{} {} {}\n", header, hex::encode(nonce), hex::encode(ciphertext));
    assert!(matches!(decrypt(&root(), &text), Err(KeyStoreError::PublicKeyMismatch)));
}

#[test]
fn the_root_credential_must_be_64_hex_characters() {
    assert!(RootKey::from_hex(&format!("{}\n", ROOT_HEX)).is_ok(), "a trailing newline is fine");
    assert!(RootKey::from_hex(&format!("  {}  ", ROOT_HEX.to_uppercase())).is_ok());
    for bad in ["", "abc", &ROOT_HEX[..62], &format!("{}00", ROOT_HEX), &format!("{}zz", &ROOT_HEX[..62])] {
        assert!(matches!(RootKey::from_hex(bad), Err(KeyStoreError::RootKeyMalformed)), "{:?}", bad);
    }
    assert_eq!(format!("{:?}", root()), "RootKey(..)", "never printed");
}

#[test]
fn the_root_credential_is_read_from_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("root");
    std::fs::write(&path, format!("{}\n", ROOT_HEX)).unwrap();
    assert!(RootKey::from_file(&path).is_ok());
    assert!(matches!(RootKey::from_file(&dir.path().join("missing")), Err(KeyStoreError::RootKeyUnreadable(_))));
}

#[test]
fn the_key_file_sits_beside_the_database_by_default() {
    assert_eq!(default_key_path("/data/bancwr.db"), Path::new("/data/bunker-key.enc"));
    assert_eq!(default_key_path("./data/bancwr.db"), Path::new("./data/bunker-key.enc"));
    assert_eq!(default_key_path("bancwr.db"), Path::new("bunker-key.enc"));
}

#[test]
fn writing_never_replaces_a_key_file_and_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bunker-key.enc");
    let keys = Keys::generate();
    assert_eq!(import(&root(), keys.secret_key(), &path).unwrap(), keys.public_key());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let before = std::fs::read_to_string(&path).unwrap();
    assert!(matches!(import(&root(), Keys::generate().secret_key(), &path), Err(KeyStoreError::KeyFileExists)));
    assert!(matches!(write_new(&path, "anything"), Err(KeyStoreError::KeyFileExists)));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("bunker-key.enc")]);
}

#[test]
fn reasons_name_no_path() {
    let errors = [
        KeyStoreError::RootKeyNotSet,
        KeyStoreError::RootKeyUnreadable(std::io::Error::other("/secret/path")),
        KeyStoreError::KeyFileUnreadable(std::io::Error::other("/secret/path")),
        KeyStoreError::WrongRootKeyOrTampered,
    ];
    for error in errors {
        assert!(!error.reason().contains("/secret/path"));
    }
}

fn locked(result: Unlock) -> (&'static str, Option<PublicKey>) {
    match result {
        Unlock::Locked { reason, public_key, .. } => (reason, public_key),
        Unlock::Unlocked(_) => panic!("expected locked"),
    }
}

#[test]
fn unlocks_with_the_right_credential_and_no_plain_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bunker-key.enc");
    let keys = Keys::generate();
    import(&root(), keys.secret_key(), &path).unwrap();

    match unlock(false, Ok(root()), &path) {
        Unlock::Unlocked(secret_key) => assert_eq!(secret_key.to_secret_hex(), keys.secret_key().to_secret_hex()),
        other => panic!("expected unlocked, got {:?}", other),
    }
}

#[test]
fn stays_locked_while_the_plain_key_is_configured() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bunker-key.enc");

    // Not imported yet: says to import.
    assert_eq!(locked(unlock(true, Ok(root()), &path)), (PLAIN_KEY_NOT_IMPORTED, None));

    // Imported, plain key still set: says to remove it, even with the right credential.
    let keys = Keys::generate();
    import(&root(), keys.secret_key(), &path).unwrap();
    assert_eq!(locked(unlock(true, Ok(root()), &path)), (PLAIN_KEY_STILL_SET, Some(keys.public_key())));
}

#[test]
fn stays_locked_without_a_good_credential_or_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bunker-key.enc");

    assert_eq!(locked(unlock(false, Ok(root()), &path)).0, KeyStoreError::KeyFileMissing.reason());

    let keys = Keys::generate();
    import(&root(), keys.secret_key(), &path).unwrap();
    assert_eq!(
        locked(unlock(false, Err(KeyStoreError::RootKeyNotSet), &path)),
        (KeyStoreError::RootKeyNotSet.reason(), Some(keys.public_key()))
    );
    assert_eq!(
        locked(unlock(false, Ok(RootKey::from_hex(OTHER_HEX).unwrap()), &path)).0,
        KeyStoreError::WrongRootKeyOrTampered.reason()
    );

    let mut changed = fields(&std::fs::read_to_string(&path).unwrap());
    changed[4] = flip(&changed[4], 3);
    std::fs::write(&path, join(&changed)).unwrap();
    assert_eq!(locked(unlock(false, Ok(root()), &path)).0, KeyStoreError::WrongRootKeyOrTampered.reason());

    std::fs::write(&path, "not a key file").unwrap();
    assert_eq!(locked(unlock(false, Ok(root()), &path)), (KeyStoreError::KeyFileMalformed.reason(), None));
}

#[test]
fn the_key_path_can_be_set() {
    // BANCWR_KEY_PATH overrides the default; only this test touches it.
    std::env::set_var(key_store::KEY_PATH_ENV, "/elsewhere/key.enc");
    assert_eq!(key_store::key_path("/data/bancwr.db"), Path::new("/elsewhere/key.enc"));
    std::env::set_var(key_store::KEY_PATH_ENV, " ");
    assert_eq!(key_store::key_path("/data/bancwr.db"), Path::new("/data/bunker-key.enc"));
    std::env::remove_var(key_store::KEY_PATH_ENV);
}
