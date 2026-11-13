//! The bunker's key at rest (#94): XChaCha20-Poly1305 ciphertext in the data volume, under a root
//! credential that only the bunker loads.
//!
//! - The root credential is 32 random bytes, written as 64 hex characters in the file
//!   `BANCWR_ROOT_KEY_FILE` names (`openssl rand -hex 32`). It is read from a file only, never from
//!   the environment, so it never shows in `inspect` or `/proc/<pid>/environ`.
//! - The encrypted key lives at `BANCWR_KEY_PATH`, by default `bunker-key.enc` beside the database.
//!   It is one line: `bancwr-key v1 <public key hex> <nonce hex> <ciphertext hex>`. The first three
//!   fields are the AEAD's associated data, so changing any of them fails decryption, and the
//!   decrypted key must also derive the recorded public key.
//! - `bunker import-key` writes that file once from the plain key (`BUNKER_NSEC_FILE` or
//!   `BUNKER_NSEC`). After that the bunker never reads the plain key: while it is still set, the
//!   bunker stays locked (`unlock`).
//!
//! Every error has fixed operator wording (`KeyStoreError::reason`), which `/health` and
//! `/api/bunker/status` show. It never names a path or includes key material.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use nostr::prelude::*;
use nostr::util::hex;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::Zeroizing;

/// The variable naming the root credential's file.
pub const ROOT_KEY_FILE_ENV: &str = "BANCWR_ROOT_KEY_FILE";
/// The variable naming the encrypted key file, when not the default.
pub const KEY_PATH_ENV: &str = "BANCWR_KEY_PATH";
/// The encrypted key file's name, beside the database by default.
pub const KEY_FILE_NAME: &str = "bunker-key.enc";

const MAGIC: &str = "bancwr-key";
const VERSION: &str = "v1";
const NONCE_LEN: usize = 24;
const ROOT_KEY_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum KeyStoreError {
    #[error("BANCWR_ROOT_KEY_FILE is not set")]
    RootKeyNotSet,
    #[error("could not read the root credential file: {0}")]
    RootKeyUnreadable(io::Error),
    #[error("the root credential is not 64 hexadecimal characters")]
    RootKeyMalformed,
    #[error("there is no encrypted key file")]
    KeyFileMissing,
    #[error("could not read the encrypted key file: {0}")]
    KeyFileUnreadable(io::Error),
    #[error("the encrypted key file is not a Bancwr key file")]
    KeyFileMalformed,
    #[error("an encrypted key file already exists")]
    KeyFileExists,
    #[error("could not write the encrypted key file: {0}")]
    KeyFileWrite(io::Error),
    #[error("the root credential does not decrypt the key: wrong credential, or an altered file")]
    WrongRootKeyOrTampered,
    #[error("the decrypted key does not match the public key the file records")]
    PublicKeyMismatch,
    #[error("BUNKER_NSEC or BUNKER_NSEC_FILE must be set to import a key")]
    PlainKeyNotSet,
    #[error("could not read BUNKER_NSEC_FILE: {0}")]
    PlainKeyUnreadable(io::Error),
    #[error("the plain key is not a valid nsec or hex secret key")]
    PlainKeyInvalid,
}

impl KeyStoreError {
    /// What the operator is told, on `/health` and in the status: fixed wording, no paths, no key
    /// material. The log gets `Display`, which may add the OS error.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::RootKeyNotSet => "BANCWR_ROOT_KEY_FILE is not set, so the bunker cannot decrypt its key. Set it to the root credential's file and restart.",
            Self::RootKeyUnreadable(_) => "The root credential file (BANCWR_ROOT_KEY_FILE) cannot be read. Check that it is mounted and readable by the bunker's user, then restart.",
            Self::RootKeyMalformed => "The root credential is not 64 hexadecimal characters. It is the value `openssl rand -hex 32` printed when the key was imported.",
            Self::KeyFileMissing => "There is no encrypted key file. Run `bunker import-key` once to create it, then restart.",
            Self::KeyFileUnreadable(_) => "The encrypted key file cannot be read. Check that the data volume is mounted and readable by the bunker's user, then restart.",
            Self::KeyFileMalformed => "The encrypted key file is damaged or is not a Bancwr key file. Restore it from a backup.",
            Self::KeyFileExists => "An encrypted key file already exists. Importing never replaces one.",
            Self::KeyFileWrite(_) => "The encrypted key file could not be written. Check that the data volume is writable by the bunker's user.",
            Self::WrongRootKeyOrTampered => "The root credential does not decrypt the bunker's key: either it is the wrong credential, or the encrypted key file has been altered.",
            Self::PublicKeyMismatch => "The decrypted key does not match the public key recorded in the encrypted key file, so the file has been altered.",
            Self::PlainKeyNotSet => "Set BUNKER_NSEC_FILE (or BUNKER_NSEC) to the key to import.",
            Self::PlainKeyUnreadable(_) => "The file BUNKER_NSEC_FILE names cannot be read.",
            Self::PlainKeyInvalid => "The plain key is not a valid nsec or hex secret key.",
        }
    }
}

/// Why the bunker is locked while the plain key is still set (#94, decided on the issue).
pub const PLAIN_KEY_NOT_IMPORTED: &str = "The bunker's key has not been imported. Run `bunker import-key` once, with BUNKER_NSEC_FILE (or BUNKER_NSEC) and BANCWR_ROOT_KEY_FILE set, then remove BUNKER_NSEC_FILE and BUNKER_NSEC and restart.";
pub const PLAIN_KEY_STILL_SET: &str = "The plain key is still configured. The key is already encrypted at rest, so remove BUNKER_NSEC_FILE and BUNKER_NSEC, delete every plain copy of the key, and restart.";

/// The root credential. Zeroed when dropped; never printed.
pub struct RootKey(Zeroizing<[u8; ROOT_KEY_LEN]>);

impl fmt::Debug for RootKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RootKey(..)")
    }
}

impl RootKey {
    /// From 64 hex characters, with surrounding whitespace (a trailing newline) allowed.
    pub fn from_hex(text: &str) -> Result<Self, KeyStoreError> {
        let text = text.trim();
        if text.len() != ROOT_KEY_LEN * 2 {
            return Err(KeyStoreError::RootKeyMalformed);
        }
        let mut bytes = Zeroizing::new([0u8; ROOT_KEY_LEN]);
        hex::decode_to_slice(text, bytes.as_mut()).map_err(|_| KeyStoreError::RootKeyMalformed)?;
        Ok(Self(bytes))
    }

    pub fn from_file(path: &Path) -> Result<Self, KeyStoreError> {
        let text = Zeroizing::new(fs::read_to_string(path).map_err(KeyStoreError::RootKeyUnreadable)?);
        Self::from_hex(&text)
    }

    /// From the file `BANCWR_ROOT_KEY_FILE` names.
    pub fn from_env() -> Result<Self, KeyStoreError> {
        match non_empty_env(ROOT_KEY_FILE_ENV) {
            Some(path) => Self::from_file(Path::new(&path)),
            None => Err(KeyStoreError::RootKeyNotSet),
        }
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(self.0.as_ref().into())
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The encrypted key file: `BANCWR_KEY_PATH`, or `bunker-key.enc` in the database's directory.
pub fn key_path(db_path: &str) -> PathBuf {
    non_empty_env(KEY_PATH_ENV).map(PathBuf::from).unwrap_or_else(|| default_key_path(db_path))
}

pub fn default_key_path(db_path: &str) -> PathBuf {
    match Path::new(db_path).parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(KEY_FILE_NAME),
        _ => PathBuf::from(KEY_FILE_NAME),
    }
}

fn header(public_key: &PublicKey) -> String {
    format!("{} {} {}", MAGIC, VERSION, public_key.to_hex())
}

/// The key file's contents for `secret_key` under `root`, with a fresh random nonce.
pub fn encrypt(root: &RootKey, secret_key: &SecretKey) -> String {
    let public_key = Keys::new(secret_key.clone()).public_key();
    let header = header(&public_key);
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let plaintext = Zeroizing::new(secret_key.to_secret_bytes());
    let ciphertext = root
        .cipher()
        .encrypt(&nonce, Payload { msg: plaintext.as_ref(), aad: header.as_bytes() })
        .expect("XChaCha20-Poly1305 encryption of 32 bytes cannot fail");
    format!("{} {} {}\n", header, hex::encode(nonce), hex::encode(ciphertext))
}

struct Parsed {
    header: String,
    public_key: PublicKey,
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

fn parse(text: &str) -> Result<Parsed, KeyStoreError> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [magic, version, public_key, nonce, ciphertext] = fields[..] else {
        return Err(KeyStoreError::KeyFileMalformed);
    };
    if magic != MAGIC || version != VERSION {
        return Err(KeyStoreError::KeyFileMalformed);
    }
    let parsed_key = PublicKey::from_hex(public_key).map_err(|_| KeyStoreError::KeyFileMalformed)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    hex::decode_to_slice(nonce, &mut nonce_bytes).map_err(|_| KeyStoreError::KeyFileMalformed)?;
    let ciphertext = hex::decode(ciphertext).map_err(|_| KeyStoreError::KeyFileMalformed)?;
    Ok(Parsed {
        // The header as written, so a key file with a re-encoded public key fails authentication.
        header: format!("{} {} {}", magic, version, public_key),
        public_key: parsed_key,
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// The public key a key file records, without decrypting it: for the status while locked.
pub fn recorded_public_key(text: &str) -> Result<PublicKey, KeyStoreError> {
    parse(text).map(|parsed| parsed.public_key)
}

/// Decrypts a key file's contents. Any change to the header, nonce or ciphertext, or the wrong
/// root credential, is `WrongRootKeyOrTampered`.
pub fn decrypt(root: &RootKey, text: &str) -> Result<SecretKey, KeyStoreError> {
    let parsed = parse(text)?;
    let plaintext = Zeroizing::new(
        root.cipher()
            .decrypt(XNonce::from_slice(&parsed.nonce), Payload { msg: &parsed.ciphertext, aad: parsed.header.as_bytes() })
            .map_err(|_| KeyStoreError::WrongRootKeyOrTampered)?,
    );
    let secret_key = SecretKey::from_slice(&plaintext).map_err(|_| KeyStoreError::WrongRootKeyOrTampered)?;
    if Keys::new(secret_key.clone()).public_key() != parsed.public_key {
        return Err(KeyStoreError::PublicKeyMismatch);
    }
    Ok(secret_key)
}

pub fn read_key_file(path: &Path) -> Result<String, KeyStoreError> {
    fs::read_to_string(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => KeyStoreError::KeyFileMissing,
        _ => KeyStoreError::KeyFileUnreadable(error),
    })
}

/// Writes a new key file, never replacing one. The contents go to a `0600` temporary file in the
/// same directory, are synced, and are then hard-linked into place, which fails if the file
/// exists, so a crash leaves either no key file or a complete one.
pub fn write_new(path: &Path, contents: &str) -> Result<(), KeyStoreError> {
    if path.exists() {
        return Err(KeyStoreError::KeyFileExists);
    }
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let temporary = dir.join(format!(".{}.{}.tmp", KEY_FILE_NAME, std::process::id()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)
    })();
    let _ = fs::remove_file(&temporary);
    match result {
        Ok(()) => {
            if let Ok(dir) = fs::File::open(&dir) {
                let _ = dir.sync_all();
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(KeyStoreError::KeyFileExists),
        Err(error) => Err(KeyStoreError::KeyFileWrite(error)),
    }
}

/// Whether `BUNKER_NSEC` or `BUNKER_NSEC_FILE` is set to anything.
pub fn plain_key_configured() -> bool {
    non_empty_env("BUNKER_NSEC").is_some() || non_empty_env("BUNKER_NSEC_FILE").is_some()
}

/// The plain key, for `bunker import-key` only: `BUNKER_NSEC_FILE` first, then `BUNKER_NSEC`, as
/// 0.1.0 read it.
pub fn read_plain_key() -> Result<SecretKey, KeyStoreError> {
    let text = if let Some(path) = non_empty_env("BUNKER_NSEC_FILE") {
        Zeroizing::new(fs::read_to_string(path).map_err(KeyStoreError::PlainKeyUnreadable)?)
    } else if let Some(nsec) = non_empty_env("BUNKER_NSEC") {
        Zeroizing::new(nsec)
    } else {
        return Err(KeyStoreError::PlainKeyNotSet);
    };
    let text = text.trim();
    SecretKey::from_bech32(text).or_else(|_| SecretKey::parse(text)).map_err(|_| KeyStoreError::PlainKeyInvalid)
}

/// Encrypts the plain key under the root credential into a new key file. Returns its public key.
pub fn import(root: &RootKey, secret_key: &SecretKey, path: &Path) -> Result<PublicKey, KeyStoreError> {
    let contents = encrypt(root, secret_key);
    write_new(path, &contents)?;
    Ok(Keys::new(secret_key.clone()).public_key())
}

/// The bunker's key at startup.
#[derive(Debug)]
pub enum Unlock {
    Unlocked(SecretKey),
    /// Nothing signs. `reason` is fixed wording; `public_key` is the one the key file records, when
    /// it can be read.
    Locked { reason: &'static str, public_key: Option<PublicKey>, error: Option<KeyStoreError> },
}

/// Works out the key state from what is configured: the plain key never unlocks the bunker, and
/// while it is set the bunker stays locked (#94).
pub fn unlock(plain_key_configured: bool, root: Result<RootKey, KeyStoreError>, path: &Path) -> Unlock {
    let file = read_key_file(path);
    let public_key = file.as_ref().ok().and_then(|text| recorded_public_key(text).ok());

    if plain_key_configured {
        let reason = match file {
            Err(KeyStoreError::KeyFileMissing) => PLAIN_KEY_NOT_IMPORTED,
            _ => PLAIN_KEY_STILL_SET,
        };
        return Unlock::Locked { reason, public_key, error: None };
    }

    let locked = |error: KeyStoreError| Unlock::Locked { reason: error.reason(), public_key, error: Some(error) };
    let text = match file {
        Ok(text) => text,
        Err(error) => return locked(error),
    };
    let root = match root {
        Ok(root) => root,
        Err(error) => return locked(error),
    };
    match decrypt(&root, &text) {
        Ok(secret_key) => Unlock::Unlocked(secret_key),
        Err(error) => locked(error),
    }
}

/// `unlock` from the environment.
pub fn unlock_from_env(db_path: &str) -> Unlock {
    unlock(plain_key_configured(), RootKey::from_env(), &key_path(db_path))
}
