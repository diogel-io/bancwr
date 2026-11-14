// Backup archives (#95): sealing and opening, every way an archive is refused, the snapshot, and
// placing a restore into an empty data directory without ever writing on a refusal.
use bunker::backup::{self, BackupError, Contents, Cost, Passphrase};
use bunker::db::{ClientMetadata, Database};
use bunker::key_store::{self, KeyStoreError, RootKey};
use bunker::migrations::LATEST_VERSION;
use bunker::registry::Role;
use chrono::{Duration, Utc};
use nostr::prelude::*;
use std::path::{Path, PathBuf};

/// Cheap enough for many tests; the binary always uses `Cost::DEFAULT`.
const FAST: Cost = Cost { log_n: 10, r: 8, p: 1 };
const ROOT_HEX: &str = "8f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";
const WRONG_HEX: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";

fn passphrase() -> Passphrase {
    Passphrase::new("correct horse battery staple").unwrap()
}

fn root() -> RootKey {
    RootKey::from_hex(ROOT_HEX).unwrap()
}

fn contents_for(keys: &Keys) -> Contents {
    Contents {
        database: b"not really a database".to_vec(),
        key_file: key_store::encrypt(&root(), keys.secret_key()),
        public_key: keys.public_key(),
        schema_version: LATEST_VERSION,
        created_at: Utc::now(),
    }
}

fn expect_err<T: std::fmt::Debug>(result: Result<T, BackupError>) -> BackupError {
    result.expect_err("should be refused")
}

fn header_end(archive: &[u8]) -> usize {
    archive.iter().position(|&b| b == b'\n').unwrap()
}

/// The archive with one header field replaced.
fn with_field(archive: &[u8], index: usize, value: &str) -> Vec<u8> {
    let end = header_end(archive);
    let line = std::str::from_utf8(&archive[..end]).unwrap();
    let mut fields: Vec<String> = line.split(' ').map(str::to_string).collect();
    fields[index] = value.to_string();
    let mut out = fields.join(" ").into_bytes();
    out.extend_from_slice(&archive[end..]);
    out
}

#[test]
fn an_archive_opens_with_its_passphrase() {
    let keys = Keys::generate();
    let contents = contents_for(&keys);
    let archive = backup::seal(&passphrase(), &contents, FAST).unwrap();

    let opened = backup::open(&passphrase(), &archive).unwrap();
    assert_eq!(opened.database, contents.database);
    assert_eq!(opened.key_file, contents.key_file);
    assert_eq!(opened.public_key, keys.public_key());
    assert_eq!(opened.schema_version, LATEST_VERSION);
    assert_eq!(opened.created_at.timestamp(), contents.created_at.timestamp());

    // The header names the cost, and the archive never holds the key in the clear.
    let header = std::str::from_utf8(&archive[..header_end(&archive)]).unwrap();
    assert!(header.starts_with("bancwr-backup v1 10 8 1 "), "{}", header);
    let text = String::from_utf8_lossy(&archive);
    assert!(!text.contains(&keys.secret_key().to_secret_hex()));
    assert!(!text.contains(&keys.secret_key().to_bech32().unwrap()));

    // A fresh salt and nonce every time.
    assert_ne!(backup::seal(&passphrase(), &contents, FAST).unwrap(), archive);
}

#[test]
fn a_wrong_passphrase_is_refused() {
    let archive = backup::seal(&passphrase(), &contents_for(&Keys::generate()), FAST).unwrap();
    let wrong = Passphrase::new("correct horse battery stapler").unwrap();
    assert!(matches!(expect_err(backup::open(&wrong, &archive)), BackupError::WrongPassphraseOrTampered));
}

#[test]
fn any_altered_byte_is_refused() {
    let archive = backup::seal(&passphrase(), &contents_for(&Keys::generate()), FAST).unwrap();
    let end = header_end(&archive);

    // Every header field the AEAD authenticates: salt, public key, schema version, time.
    let other_key = Keys::generate().public_key().to_hex();
    for (index, value) in [(5, "00".repeat(32).as_str()), (6, other_key.as_str()), (7, "1"), (8, "2020-01-01T00:00:00Z")] {
        let altered = with_field(&archive, index, value);
        assert!(
            matches!(expect_err(backup::open(&passphrase(), &altered)), BackupError::WrongPassphraseOrTampered),
            "field {}",
            index
        );
    }

    // One bit in the nonce, in the ciphertext, and its last byte.
    for position in [end + 1, end + 1 + 24 + 3, archive.len() - 1] {
        let mut altered = archive.clone();
        altered[position] ^= 1;
        assert!(
            matches!(expect_err(backup::open(&passphrase(), &altered)), BackupError::WrongPassphraseOrTampered),
            "byte {}",
            position
        );
    }

    // Truncated anywhere.
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive[..archive.len() - 1])), BackupError::WrongPassphraseOrTampered));
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive[..end + 10])), BackupError::NotAnArchive));
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive[..end])), BackupError::NotAnArchive));
    assert!(matches!(expect_err(backup::open(&passphrase(), b"")), BackupError::NotAnArchive));
}

#[test]
fn a_foreign_or_later_archive_is_refused() {
    let archive = backup::seal(&passphrase(), &contents_for(&Keys::generate()), FAST).unwrap();
    assert!(matches!(expect_err(backup::open(&passphrase(), &with_field(&archive, 0, "other"))), BackupError::NotAnArchive));
    assert!(matches!(expect_err(backup::open(&passphrase(), &with_field(&archive, 1, "v2"))), BackupError::UnsupportedVersion));
    assert!(matches!(expect_err(backup::open(&passphrase(), &with_field(&archive, 7, "x"))), BackupError::NotAnArchive));
    assert!(matches!(expect_err(backup::open(&passphrase(), b"SQLite format 3\0")), BackupError::NotAnArchive));
}

#[test]
fn a_cost_above_the_ceiling_is_refused_before_deriving() {
    let archive = backup::seal(&passphrase(), &contents_for(&Keys::generate()), FAST).unwrap();
    for (index, value) in [(2, "21"), (2, "0"), (3, "9"), (4, "5"), (4, "0")] {
        let altered = with_field(&archive, index, value);
        assert!(matches!(expect_err(backup::open(&passphrase(), &altered)), BackupError::CostTooHigh), "{} = {}", index, value);
    }
    assert!(Cost::DEFAULT.within_ceiling());
}

#[test]
fn a_key_file_for_another_key_is_refused() {
    let mut contents = contents_for(&Keys::generate());
    contents.key_file = key_store::encrypt(&root(), Keys::generate().secret_key());
    let archive = backup::seal(&passphrase(), &contents, FAST).unwrap();
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive)), BackupError::KeyMismatch));

    contents.key_file = "not a key file".to_string();
    let archive = backup::seal(&passphrase(), &contents, FAST).unwrap();
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive)), BackupError::KeyMismatch));
}

#[test]
fn a_newer_schema_is_refused() {
    let mut contents = contents_for(&Keys::generate());
    contents.schema_version = LATEST_VERSION + 1;
    let archive = backup::seal(&passphrase(), &contents, FAST).unwrap();
    assert!(matches!(expect_err(backup::open(&passphrase(), &archive)), BackupError::SchemaTooNew));
}

#[test]
fn a_short_passphrase_is_refused() {
    assert!(matches!(Passphrase::new("fifteen chars..").unwrap_err(), BackupError::PassphraseTooShort));
    assert!(matches!(Passphrase::new("   short   \n").unwrap_err(), BackupError::PassphraseTooShort));
    // Surrounding whitespace is not part of it.
    let archive = backup::seal(&Passphrase::new("  correct horse battery staple\n").unwrap(), &contents_for(&Keys::generate()), FAST).unwrap();
    assert!(backup::open(&passphrase(), &archive).is_ok());
}

/// A bunker's data directory: a database with a member, an issued and a used token, an active
/// connection and a signature, and the encrypted key file.
struct DataDir {
    _dir: tempfile::TempDir,
    db: PathBuf,
    key: PathBuf,
    keys: Keys,
}

fn populated() -> DataDir {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data/bancwr.db");
    let key = dir.path().join("data/bunker-key.enc");
    let keys = Keys::generate();
    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    let alice = Keys::generate().public_key().to_hex();
    db.add_team_member("Alice", &alice, Role::Administrator).unwrap();
    db.add_team_member("Bob", &Keys::generate().public_key().to_hex(), Role::Viewer).unwrap();
    let now = Utc::now();
    let kinds = vec!["sign_event:1".to_string()];
    db.create_nip46_token("hash-used", &alice, &alice, "Used", &kinds, now, now + Duration::hours(1)).unwrap();
    db.create_nip46_token("hash-open", &alice, &alice, "Open", &kinds, now, now + Duration::hours(1)).unwrap();
    let metadata = ClientMetadata { name: Some("App".into()), url: None, image: None };
    db.redeem_nip46_token("hash-used", &Keys::generate().public_key().to_hex(), None, &metadata, now).unwrap().unwrap();
    db.log_signing_event("event", "client", None, None, 1, now).unwrap();
    // Left open, as a running bunker leaves it: recent writes are still in the WAL.
    std::mem::forget(db);
    key_store::write_new(&key, &key_store::encrypt(&root(), keys.secret_key())).unwrap();
    DataDir { _dir: dir, db: db_path, key, keys }
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_snapshot_restores_into_an_empty_directory_and_revokes_connections() {
    let data = populated();
    let (contents, counts) = backup::snapshot(&data.db, &data.key, Utc::now()).unwrap();
    assert_eq!((counts.members, counts.connections, counts.signatures), (2, 1, 1));
    assert_eq!(contents.public_key, data.keys.public_key());
    assert_eq!(contents.schema_version, LATEST_VERSION);
    assert_eq!(contents.key_file, std::fs::read_to_string(&data.key).unwrap());
    // The temporary copy is gone.
    assert!(files_in(data.db.parent().unwrap()).iter().all(|name| !name.ends_with(".tmp")));

    let archive = backup::seal(&passphrase(), &contents, FAST).unwrap();
    let opened = backup::open(&passphrase(), &archive).unwrap();

    let target = tempfile::tempdir().unwrap();
    let db_path = target.path().join("data/bancwr.db");
    let key_path = target.path().join("data/bunker-key.enc");
    backup::place(&opened, Ok(root()), &db_path, &key_path).unwrap();
    assert_eq!(files_in(&target.path().join("data")), vec!["bancwr.db", "bunker-key.enc"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&db_path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(target.path().join("data")).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let unlocked = key_store::decrypt(&root(), &std::fs::read_to_string(&key_path).unwrap()).unwrap();
    assert_eq!(Keys::new(unlocked).public_key(), data.keys.public_key());

    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    assert_eq!(db.revoke_all_for_restore(Utc::now()).unwrap(), (1, 1));
    let members = db.get_team_members().unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.iter().any(|m| m.name == "Bob" && m.role() == Some(Role::Viewer)));
    let connections = db.list_nip46_connections(None).unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].revoked_reason.as_deref(), Some(backup::RESTORED_REASON));
    assert_eq!(connections[0].revoked_by, None);
    assert!(db.list_nip46_tokens().unwrap().iter().all(|t| t.revoked_at.is_some() || t.used_at.is_some()));
    assert_eq!(db.get_recent_logs(10).unwrap().len(), 1);
    // Nothing left to revoke a second time.
    assert_eq!(db.revoke_all_for_restore(Utc::now()).unwrap(), (0, 0));
}

#[test]
fn a_restore_writes_nothing_when_refused() {
    let data = populated();
    let (contents, _) = backup::snapshot(&data.db, &data.key, Utc::now()).unwrap();
    let target = tempfile::tempdir().unwrap();
    let dir = target.path().join("data");
    let db_path = dir.join("bancwr.db");
    let key_path = dir.join("bunker-key.enc");

    // Wrong, missing and malformed root credentials.
    let wrong = backup::place(&contents, Ok(RootKey::from_hex(WRONG_HEX).unwrap()), &db_path, &key_path);
    assert!(matches!(wrong.unwrap_err(), BackupError::RootKeyDoesNotDecrypt));
    let missing = backup::place(&contents, Err(KeyStoreError::RootKeyNotSet), &db_path, &key_path);
    assert_eq!(missing.unwrap_err().reason(), KeyStoreError::RootKeyNotSet.reason());
    assert!(files_in(&dir).is_empty());

    // A damaged database.
    let mut damaged = Contents { database: contents.database.clone(), key_file: contents.key_file.clone(), ..contents_for(&data.keys) };
    damaged.schema_version = contents.schema_version;
    // The schema page's b-tree header, after the 100-byte file header: still "SQLite format 3".
    damaged.database[100..200].fill(0xff);
    let result = backup::place(&damaged, Ok(root()), &db_path, &key_path);
    assert!(matches!(result.unwrap_err(), BackupError::DatabaseDamaged));
    assert!(files_in(&dir).is_empty(), "{:?}", files_in(&dir));

    // Anything already there: a database, its WAL, or a key file.
    for name in ["bancwr.db", "bancwr.db-wal", "bunker-key.enc"] {
        std::fs::write(dir.join(name), b"x").unwrap();
        let result = backup::place(&contents, Ok(root()), &db_path, &key_path);
        assert!(matches!(result.unwrap_err(), BackupError::DataDirNotEmpty), "{}", name);
        assert_eq!(files_in(&dir), vec![name.to_string()]);
        std::fs::remove_file(dir.join(name)).unwrap();
    }
}

#[test]
fn a_snapshot_needs_the_database_and_the_key_file() {
    let data = populated();
    let elsewhere = data.db.with_file_name("other.db");
    assert!(matches!(backup::snapshot(&elsewhere, &data.key, Utc::now()).unwrap_err(), BackupError::NoDatabase));
    std::fs::remove_file(&data.key).unwrap();
    assert!(matches!(backup::snapshot(&data.db, &data.key, Utc::now()).unwrap_err(), BackupError::NoKeyFile));
}
