//! Backup and restore (#95): one encrypted archive holding a consistent snapshot of the database
//! and the encrypted key file, and a restore that revokes every NIP-46 connection.
//!
//! - The backup passphrase is at least 16 characters in the file `BANCWR_BACKUP_KEY_FILE` names.
//!   It is read from a file only, like the root credential, and must not be the root credential:
//!   the two are kept apart, so an archive and its passphrase together still hold no usable key.
//! - An archive is a header line, then a 24-byte nonce and the XChaCha20-Poly1305 ciphertext:
//!
//!   `bancwr-backup v1 <log_n> <r> <p> <salt hex> <public key hex> <schema version> <created at>`
//!
//!   The archive key is scrypt of the passphrase with the salt and the parameters given. The whole
//!   header line is the AEAD's associated data, so changing any field fails decryption.
//! - The plaintext is the database snapshot and the key file's text, each prefixed with its length
//!   as eight big-endian bytes. The key file is copied as it is, so the archive holds the signing
//!   key only as ciphertext under the root credential, which is never in an archive.
//!
//! Every error has fixed operator wording (`BackupError::reason`), which never names a path or
//! includes key material.

use crate::key_store::{self, KeyStoreError, RootKey};
use crate::migrations;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chrono::{DateTime, Utc};
use nostr::prelude::*;
use nostr::util::hex;
use rusqlite::{Connection, OpenFlags};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use zeroize::Zeroizing;

/// The variable naming the backup passphrase's file.
pub const BACKUP_KEY_FILE_ENV: &str = "BANCWR_BACKUP_KEY_FILE";
/// The shortest passphrase accepted, in characters.
pub const MIN_PASSPHRASE_CHARS: usize = 16;
/// The reason restored connections are revoked with.
pub const RESTORED_REASON: &str = "restored";

const MAGIC: &str = "bancwr-backup";
const VERSION: &str = "v1";
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32;
/// The longest header line read before it is called not an archive.
const MAX_HEADER_LEN: usize = 512;

/// scrypt's cost. Written into every archive, so a later default never breaks an older archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
}

impl Cost {
    /// 128 MiB and about a second: paid once per backup and once per restore.
    pub const DEFAULT: Cost = Cost { log_n: 17, r: 8, p: 1 };

    /// The most a restore will spend, so a crafted header cannot exhaust memory or time: 1 GiB.
    pub fn within_ceiling(&self) -> bool {
        (1..=20).contains(&self.log_n) && (1..=8).contains(&self.r) && (1..=4).contains(&self.p)
    }
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("BANCWR_BACKUP_KEY_FILE is not set")]
    PassphraseNotSet,
    #[error("could not read the backup passphrase file: {0}")]
    PassphraseUnreadable(io::Error),
    #[error("the backup passphrase is shorter than {MIN_PASSPHRASE_CHARS} characters")]
    PassphraseTooShort,
    #[error("the backup passphrase is the root credential")]
    PassphraseIsRootKey,
    #[error("standard output is a terminal")]
    StdoutIsTerminal,
    #[error("standard input is a terminal")]
    StdinIsTerminal,
    #[error("there is no database to back up")]
    NoDatabase,
    #[error("there is no encrypted key file to back up")]
    NoKeyFile,
    #[error("could not take the database snapshot: {0}")]
    Snapshot(String),
    #[error("could not read the archive: {0}")]
    ReadArchive(io::Error),
    #[error("could not write the archive: {0}")]
    WriteArchive(io::Error),
    #[error("not a Bancwr backup archive")]
    NotAnArchive,
    #[error("unsupported archive version")]
    UnsupportedVersion,
    #[error("the archive asks for more scrypt memory or time than a restore allows")]
    CostTooHigh,
    #[error("the passphrase does not decrypt the archive: wrong passphrase, or an altered archive")]
    WrongPassphraseOrTampered,
    #[error("the archive's database fails its integrity check")]
    DatabaseDamaged,
    #[error("the archive's key file does not match the archive's public key")]
    KeyMismatch,
    #[error("the archive's database is newer than this bunker")]
    SchemaTooNew,
    #[error("the data directory already holds a database or key file")]
    DataDirNotEmpty,
    #[error("{0}")]
    RootKey(KeyStoreError),
    #[error("the root credential does not decrypt the archived key")]
    RootKeyDoesNotDecrypt,
    #[error("could not write the restored files: {0}")]
    WriteRestore(io::Error),
    #[error("the restored database could not be opened: {0}")]
    OpenRestored(String),
}

impl BackupError {
    /// What the operator is told: fixed wording, no paths, no key material.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::PassphraseNotSet => "BANCWR_BACKUP_KEY_FILE is not set. Set it to the backup passphrase's file.",
            Self::PassphraseUnreadable(_) => "The backup passphrase file (BANCWR_BACKUP_KEY_FILE) cannot be read. Check that it is mounted and readable by the bunker's user.",
            Self::PassphraseTooShort => "The backup passphrase is shorter than 16 characters.",
            Self::PassphraseIsRootKey => "The backup passphrase is the root credential. Use a different value, so that an archive and its passphrase together still hold no usable key.",
            Self::StdoutIsTerminal => "The archive is written to standard output. Redirect it to a file.",
            Self::StdinIsTerminal => "The archive is read from standard input. Redirect it from a file.",
            Self::NoDatabase => "There is no database to back up. Check DATABASE_PATH and that the data volume is mounted.",
            Self::NoKeyFile => "There is no encrypted key file to back up. Run `bunker import-key` first.",
            Self::Snapshot(_) => "The database snapshot could not be taken. Check that the data volume is writable by the bunker's user and has free space.",
            Self::ReadArchive(_) => "The archive could not be read.",
            Self::WriteArchive(_) => "The archive could not be written.",
            Self::NotAnArchive => "This is not a Bancwr backup archive, or it is truncated.",
            Self::UnsupportedVersion => "This archive was made by a version of Bancwr that this bunker cannot read.",
            Self::CostTooHigh => "The archive asks for more memory or time than a restore allows, so it was not made by Bancwr.",
            Self::WrongPassphraseOrTampered => "The backup passphrase does not decrypt the archive: either it is the wrong passphrase, or the archive has been altered.",
            Self::DatabaseDamaged => "The database in the archive fails its integrity check. Use another archive.",
            Self::KeyMismatch => "The key file in the archive does not match the archive's public key, so the archive has been altered.",
            Self::SchemaTooNew => "The archive comes from a newer Bancwr. Restore it with that version or later.",
            Self::DataDirNotEmpty => "The data directory already holds a database or key file. Restore only into an empty data directory: stop the bunker and move its data aside first.",
            Self::RootKey(error) => error.reason(),
            Self::RootKeyDoesNotDecrypt => "The root credential does not decrypt the key in the archive. Restore with the root credential the key was imported under.",
            Self::WriteRestore(_) => "The restored files could not be written. Check that the data volume is writable by the bunker's user and has free space.",
            Self::OpenRestored(_) => "The restored database could not be opened.",
        }
    }
}

/// The backup passphrase. Zeroed when dropped; never printed.
pub struct Passphrase(Zeroizing<String>);

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(..)")
    }
}

impl Passphrase {
    /// From text, with surrounding whitespace (a trailing newline) removed.
    pub fn new(text: &str) -> Result<Self, BackupError> {
        let text = text.trim();
        if text.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(BackupError::PassphraseTooShort);
        }
        Ok(Self(Zeroizing::new(text.to_string())))
    }

    /// From the file `BANCWR_BACKUP_KEY_FILE` names. Refused when it holds the same value as the
    /// file `BANCWR_ROOT_KEY_FILE` names, when that can be read.
    pub fn from_env() -> Result<Self, BackupError> {
        let path = non_empty_env(BACKUP_KEY_FILE_ENV).ok_or(BackupError::PassphraseNotSet)?;
        let text = Zeroizing::new(fs::read_to_string(path).map_err(BackupError::PassphraseUnreadable)?);
        let passphrase = Self::new(&text)?;
        if let Some(root_path) = non_empty_env(key_store::ROOT_KEY_FILE_ENV) {
            if let Ok(root) = fs::read_to_string(root_path).map(Zeroizing::new) {
                if root.trim().eq_ignore_ascii_case(&passphrase.0) {
                    return Err(BackupError::PassphraseIsRootKey);
                }
            }
        }
        Ok(passphrase)
    }

    fn derive(&self, salt: &[u8], cost: Cost) -> Result<Zeroizing<[u8; KEY_LEN]>, BackupError> {
        let params = scrypt::Params::new(cost.log_n, cost.r, cost.p, KEY_LEN).map_err(|_| BackupError::CostTooHigh)?;
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        scrypt::scrypt(self.0.as_bytes(), salt, &params, key.as_mut()).map_err(|_| BackupError::CostTooHigh)?;
        Ok(key)
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// What an archive holds, decrypted.
pub struct Contents {
    pub database: Vec<u8>,
    pub key_file: String,
    pub public_key: PublicKey,
    pub schema_version: usize,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for Contents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Contents")
            .field("database", &format_args!("{} bytes", self.database.len()))
            .field("public_key", &self.public_key)
            .field("schema_version", &self.schema_version)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

/// Encrypts `contents` under `passphrase` into an archive, with a fresh salt and nonce.
pub fn seal(passphrase: &Passphrase, contents: &Contents, cost: Cost) -> Result<Vec<u8>, BackupError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = passphrase.derive(&salt, cost)?;
    let header = format!(
        "{} {} {} {} {} {} {} {} {}",
        MAGIC,
        VERSION,
        cost.log_n,
        cost.r,
        cost.p,
        hex::encode(salt),
        contents.public_key.to_hex(),
        contents.schema_version,
        contents.created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    let mut plaintext = Zeroizing::new(Vec::with_capacity(contents.database.len() + contents.key_file.len() + 16));
    plaintext.extend_from_slice(&(contents.database.len() as u64).to_be_bytes());
    plaintext.extend_from_slice(&contents.database);
    plaintext.extend_from_slice(&(contents.key_file.len() as u64).to_be_bytes());
    plaintext.extend_from_slice(contents.key_file.as_bytes());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = XChaCha20Poly1305::new(key.as_ref().into())
        .encrypt(&nonce, Payload { msg: &plaintext, aad: header.as_bytes() })
        .map_err(|_| BackupError::WriteArchive(io::Error::other("encryption failed")))?;
    let mut archive = Vec::with_capacity(header.len() + 1 + NONCE_LEN + ciphertext.len());
    archive.extend_from_slice(header.as_bytes());
    archive.push(b'\n');
    archive.extend_from_slice(&nonce);
    archive.extend_from_slice(&ciphertext);
    Ok(archive)
}

struct Header<'a> {
    line: &'a str,
    cost: Cost,
    salt: [u8; SALT_LEN],
    public_key: PublicKey,
    schema_version: usize,
    created_at: DateTime<Utc>,
}

fn parse_header(archive: &[u8]) -> Result<(Header<'_>, &[u8]), BackupError> {
    let end = archive.iter().take(MAX_HEADER_LEN).position(|&b| b == b'\n').ok_or(BackupError::NotAnArchive)?;
    let line = std::str::from_utf8(&archive[..end]).map_err(|_| BackupError::NotAnArchive)?;
    let fields: Vec<&str> = line.split(' ').collect();
    if fields.first() != Some(&MAGIC) {
        return Err(BackupError::NotAnArchive);
    }
    if fields.get(1) != Some(&VERSION) {
        return Err(BackupError::UnsupportedVersion);
    }
    let [_, _, log_n, r, p, salt, public_key, schema, created_at] = fields[..] else {
        return Err(BackupError::NotAnArchive);
    };
    let bad = |_| BackupError::NotAnArchive;
    let cost = Cost {
        log_n: log_n.parse().map_err(|_| BackupError::NotAnArchive)?,
        r: r.parse().map_err(|_| BackupError::NotAnArchive)?,
        p: p.parse().map_err(|_| BackupError::NotAnArchive)?,
    };
    if !cost.within_ceiling() {
        return Err(BackupError::CostTooHigh);
    }
    let mut salt_bytes = [0u8; SALT_LEN];
    hex::decode_to_slice(salt, &mut salt_bytes).map_err(bad)?;
    let header = Header {
        line,
        cost,
        salt: salt_bytes,
        public_key: PublicKey::from_hex(public_key).map_err(|_| BackupError::NotAnArchive)?,
        schema_version: schema.parse().map_err(|_| BackupError::NotAnArchive)?,
        created_at: DateTime::parse_from_rfc3339(created_at).map_err(|_| BackupError::NotAnArchive)?.with_timezone(&Utc),
    };
    Ok((header, &archive[end + 1..]))
}

fn take_field(bytes: &[u8]) -> Result<(&[u8], &[u8]), BackupError> {
    if bytes.len() < 8 {
        return Err(BackupError::WrongPassphraseOrTampered);
    }
    let (len, rest) = bytes.split_at(8);
    let len = u64::from_be_bytes(len.try_into().expect("eight bytes")) as usize;
    if rest.len() < len {
        return Err(BackupError::WrongPassphraseOrTampered);
    }
    Ok(rest.split_at(len))
}

/// Decrypts an archive and checks that its parts agree: the key file must record the archive's
/// public key, and the schema must be one this bunker knows. Does not touch the disk.
pub fn open(passphrase: &Passphrase, archive: &[u8]) -> Result<Contents, BackupError> {
    let (header, body) = parse_header(archive)?;
    if body.len() < NONCE_LEN {
        return Err(BackupError::NotAnArchive);
    }
    let (nonce, ciphertext) = body.split_at(NONCE_LEN);
    let key = passphrase.derive(&header.salt, header.cost)?;
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new(key.as_ref().into())
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ciphertext, aad: header.line.as_bytes() })
            .map_err(|_| BackupError::WrongPassphraseOrTampered)?,
    );
    let (database, rest) = take_field(&plaintext)?;
    let (key_file, rest) = take_field(rest)?;
    if !rest.is_empty() {
        return Err(BackupError::WrongPassphraseOrTampered);
    }
    let key_file = String::from_utf8(key_file.to_vec()).map_err(|_| BackupError::KeyMismatch)?;
    match key_store::recorded_public_key(&key_file) {
        Ok(recorded) if recorded == header.public_key => {}
        _ => return Err(BackupError::KeyMismatch),
    }
    if header.schema_version > migrations::LATEST_VERSION {
        return Err(BackupError::SchemaTooNew);
    }
    Ok(Contents {
        database: database.to_vec(),
        key_file,
        public_key: header.public_key,
        schema_version: header.schema_version,
        created_at: header.created_at,
    })
}

/// What a snapshot holds, for the summary `bunker backup` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub members: u64,
    pub connections: u64,
    pub signatures: u64,
}

/// A file removed when dropped, for the plain database copies made on the way.
struct Temporary(PathBuf);

impl Temporary {
    /// An empty `0600` file in `dir`, which must not exist yet.
    fn create(dir: &Path, label: &str) -> io::Result<(Self, fs::File)> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = dir.join(format!(".bancwr-{}.{}.{}.tmp", label, std::process::id(), nanos));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        Ok((Self(path), file))
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn directory_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn snapshot_error(error: impl fmt::Display) -> BackupError {
    BackupError::Snapshot(error.to_string())
}

/// Opens one of the plain copies made on the way, never creating it. Read-write, so a copy left in
/// WAL mode needs no `-shm` file to exist first.
fn open_copy(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| e.to_string())
}

/// Checks a database file: `PRAGMA integrity_check` must say `ok`. Returns its schema version.
fn check_database(path: &Path) -> Result<usize, String> {
    let conn = open_copy(path)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err(integrity);
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    Ok(version as usize)
}

/// A consistent snapshot of the database at `db_path`, safe while the bunker runs: `VACUUM INTO` a
/// `0600` temporary file beside it, which is read back and deleted. With the key file, ready to
/// seal.
pub fn snapshot(db_path: &Path, key_path: &Path, now: DateTime<Utc>) -> Result<(Contents, Counts), BackupError> {
    if !db_path.exists() {
        return Err(BackupError::NoDatabase);
    }
    let key_file = match key_store::read_key_file(key_path) {
        Ok(text) => text,
        Err(KeyStoreError::KeyFileMissing) => return Err(BackupError::NoKeyFile),
        Err(error) => return Err(BackupError::RootKey(error)),
    };
    let public_key = key_store::recorded_public_key(&key_file).map_err(BackupError::RootKey)?;

    let (copy, _) = Temporary::create(&directory_of(db_path), "backup").map_err(snapshot_error)?;
    {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(snapshot_error)?;
        conn.busy_timeout(Duration::from_secs(30)).map_err(snapshot_error)?;
        let target = copy.0.to_str().ok_or_else(|| snapshot_error("the data directory's path is not UTF-8"))?;
        conn.execute("VACUUM INTO ?1", [target]).map_err(snapshot_error)?;
    }
    // One self-contained file: rollback journal, so a restore needs no `-wal` beside it.
    open_copy(&copy.0)
        .and_then(|conn| conn.pragma_update(None, "journal_mode", "DELETE").map_err(|e| e.to_string()))
        .map_err(snapshot_error)?;
    let schema_version = check_database(&copy.0).map_err(snapshot_error)?;
    let counts = {
        let conn = open_copy(&copy.0).map_err(snapshot_error)?;
        let count = |table: &str| -> Result<u64, BackupError> {
            let exists: bool = conn
                .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)", [table], |row| row.get(0))
                .map_err(snapshot_error)?;
            if !exists {
                return Ok(0);
            }
            conn.query_row(&format!("SELECT count(*) FROM {}", table), [], |row| row.get::<_, i64>(0))
                .map(|n| n as u64)
                .map_err(snapshot_error)
        };
        Counts { members: count("team_members")?, connections: count("nip46_connections")?, signatures: count("signing_logs")? }
    };
    let database = fs::read(&copy.0).map_err(snapshot_error)?;
    drop(copy);
    Ok((Contents { database, key_file, public_key, schema_version, created_at: now }, counts))
}

/// The files a data directory must not hold before a restore.
fn occupied(db_path: &Path, key_path: &Path) -> bool {
    let with_suffix = |suffix: &str| {
        let mut name = db_path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    [db_path.to_path_buf(), with_suffix("-wal"), with_suffix("-shm"), with_suffix("-journal"), key_path.to_path_buf()]
        .iter()
        .any(|path| path.exists())
}

/// Puts an opened archive's database and key file into an empty data directory, after checking
/// that the root credential decrypts the key and that the database passes its integrity check.
/// Nothing is written until every check passes; a failure while writing removes what it wrote.
/// The caller then opens the database and revokes its connections.
pub fn place(contents: &Contents, root: Result<RootKey, KeyStoreError>, db_path: &Path, key_path: &Path) -> Result<(), BackupError> {
    if occupied(db_path, key_path) {
        return Err(BackupError::DataDirNotEmpty);
    }
    let root = root.map_err(BackupError::RootKey)?;
    match key_store::decrypt(&root, &contents.key_file) {
        Ok(_) => {}
        Err(KeyStoreError::WrongRootKeyOrTampered) | Err(KeyStoreError::PublicKeyMismatch) => {
            return Err(BackupError::RootKeyDoesNotDecrypt)
        }
        Err(error) => return Err(BackupError::RootKey(error)),
    }

    let dir = directory_of(db_path);
    if !dir.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).map_err(BackupError::WriteRestore)?;
    }
    let (staged, mut file) = Temporary::create(&dir, "restore").map_err(BackupError::WriteRestore)?;
    file.write_all(&contents.database).and_then(|_| file.sync_all()).map_err(BackupError::WriteRestore)?;
    drop(file);
    match check_database(&staged.0) {
        Ok(version) if version == contents.schema_version => {}
        _ => return Err(BackupError::DatabaseDamaged),
    }
    // A hard link fails if the database appeared meanwhile, so nothing is ever replaced.
    fs::hard_link(&staged.0, db_path).map_err(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => BackupError::DataDirNotEmpty,
        _ => BackupError::WriteRestore(error),
    })?;
    drop(staged);
    if let Err(error) = key_store::write_new(key_path, &contents.key_file) {
        let _ = fs::remove_file(db_path);
        return Err(match error {
            KeyStoreError::KeyFileExists => BackupError::DataDirNotEmpty,
            KeyStoreError::KeyFileWrite(error) => BackupError::WriteRestore(error),
            other => BackupError::RootKey(other),
        });
    }
    if let Ok(dir) = fs::File::open(&dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}
