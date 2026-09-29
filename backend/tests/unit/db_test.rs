use bunker::db::{Database, SeedOutcome};
use bunker::registry::Role;
use chrono::Utc;
use nostr::prelude::*;

/// A valid key in the vault's stored form, lowercase hex.
fn new_pubkey() -> String {
    Keys::generate().public_key().to_hex()
}
use tempfile::NamedTempFile;

#[test]
fn test_db_init_and_tables() {
    let _db = Database::new(":memory:").expect("Failed to create in-memory database");
}

#[test]
fn test_db_log_signing_event() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let event_id = "test_event_id";
    let pubkey = "test_pubkey";
    let kind = 1;
    let now = Utc::now();

    db.log_signing_event(event_id, pubkey, kind, now).expect("Failed to log event");

    let logs = db.get_recent_logs(10).expect("Failed to get logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].event_id, event_id);
    assert_eq!(logs[0].pubkey, pubkey);
    assert_eq!(logs[0].event_kind, kind);
}

#[test]
fn test_db_config_storage() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let key = "test_key";
    let value = "test_value";

    db.set_config(key, value).expect("Failed to set config");
    let retrieved = db.get_config(key).expect("Failed to get config");
    assert_eq!(retrieved, Some(value.to_string()));

    // Test update
    let new_value = "new_value";
    db.set_config(key, new_value).expect("Failed to update config");
    let retrieved = db.get_config(key).expect("Failed to get config");
    assert_eq!(retrieved, Some(new_value.to_string()));
}

#[test]
fn test_db_get_config_non_existent() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let retrieved = db.get_config("non_existent").expect("Failed to get config");
    assert_eq!(retrieved, None);
}

#[test]
fn test_db_recent_logs_ordering() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let now = Utc::now();

    db.log_signing_event("event1", "pub1", 1, now).unwrap();
    db.log_signing_event("event2", "pub2", 2, now + chrono::Duration::seconds(1)).unwrap();

    let logs = db.get_recent_logs(10).unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].event_id, "event2");
    assert_eq!(logs[1].event_id, "event1");
}

#[test]
fn test_db_team_management() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");

    // Add member
    let alice = new_pubkey();
    let id = db.add_team_member("Alice", &alice, Role::Administrator).expect("Failed to add member");

    // Get members
    let members = db.get_team_members().expect("Failed to get members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "Alice");
    assert_eq!(members[0].pubkey, alice);
    assert_eq!(members[0].role, "administrator");
    assert_eq!(members[0].role(), Some(Role::Administrator));
    assert_eq!(members[0].id, id);

    // Remove member
    let removed = db.remove_team_member(id).expect("Failed to remove member");
    assert!(removed, "removing an existing member reports a removal");
    let members = db.get_team_members().expect("Failed to get members");
    assert_eq!(members.len(), 0);

    // Removing it again finds nothing
    let removed_again = db.remove_team_member(id).expect("Failed to remove member");
    assert!(!removed_again, "removing an unknown id reports no removal");
}

// --- Stored keys from the removed Config page write path (#42) ---

#[test]
fn test_purge_stored_key_config_removes_the_nsec_from_disk() {
    let tmp = NamedTempFile::new().expect("Failed to create temp file");
    let path = tmp.path().to_str().unwrap().to_string();
    // A distinctive value, so finding its bytes on disk can only mean the key survived.
    let nsec = "nsec1purgetestpurgetestpurgetestpurgetestpurgetestpurgetest00000";

    {
        let db = Database::new(&path).expect("Failed to create file-backed database");
        db.set_config("nsec", nsec).expect("Failed to set config");
        db.set_config("nsec_file", "/run/secrets/old_nsec").expect("Failed to set config");
        db.set_config("bunker_secret", "keep-me").expect("Failed to set config");
    }

    let db = Database::new(&path).expect("Failed to reopen database");
    let removed_nsec = db.purge_stored_key_config().expect("Failed to purge");

    assert!(removed_nsec, "reports that an nsec was stored");
    assert_eq!(db.get_config("nsec").unwrap(), None);
    assert_eq!(db.get_config("nsec_file").unwrap(), None);
    // NIP-46's secret is used, so it stays
    assert_eq!(db.get_config("bunker_secret").unwrap(), Some("keep-me".to_string()));

    // Neither the database file nor its WAL still holds the key's bytes.
    for file in [path.clone(), format!("{}-wal", path)] {
        if let Ok(bytes) = std::fs::read(&file) {
            assert!(
                !bytes.windows(nsec.len()).any(|w| w == nsec.as_bytes()),
                "{} still contains the purged nsec",
                file
            );
        }
    }
}

#[test]
fn test_purge_stored_key_config_on_a_clean_database() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    db.set_config("bunker_secret", "keep-me").expect("Failed to set config");

    let removed_nsec = db.purge_stored_key_config().expect("Failed to purge");

    assert!(!removed_nsec, "nothing to purge");
    assert_eq!(db.get_config("bunker_secret").unwrap(), Some("keep-me".to_string()));
}

// --- New checks required by the migration plan ---

#[test]
fn test_db_file_backed_persistence() {
    let tmp = NamedTempFile::new().expect("Failed to create temp file");
    let path = tmp.path().to_str().unwrap().to_string();

    {
        let db = Database::new(&path).expect("Failed to create file-backed database");
        db.set_config("persist_key", "persist_value").expect("Failed to set config");
        db.log_signing_event("evt_persist", "pubkey_persist", 1, Utc::now())
            .expect("Failed to log event");
        db.add_team_member("Bob", &new_pubkey(), Role::Signer)
            .expect("Failed to add member");
    } // db handle dropped here

    // Reopen and verify data survived
    let db2 = Database::new(&path).expect("Failed to reopen file-backed database");

    let val = db2.get_config("persist_key").expect("Failed to get config");
    assert_eq!(val, Some("persist_value".to_string()));

    let logs = db2.get_recent_logs(10).expect("Failed to get logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].event_id, "evt_persist");

    let members = db2.get_team_members().expect("Failed to get members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "Bob");
}

#[test]
fn test_db_timestamp_ordering_rfc3339() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let base = Utc::now();

    // Insert out-of-order; expect DESC ordering by RFC3339 text
    db.log_signing_event("oldest", "pub", 1, base).unwrap();
    db.log_signing_event("newest", "pub", 1, base + chrono::Duration::seconds(10)).unwrap();
    db.log_signing_event("middle", "pub", 1, base + chrono::Duration::seconds(5)).unwrap();

    let logs = db.get_recent_logs(10).unwrap();
    assert_eq!(logs[0].event_id, "newest");
    assert_eq!(logs[1].event_id, "middle");
    assert_eq!(logs[2].event_id, "oldest");
}

#[test]
fn test_db_duplicate_team_member_pubkey_rejected() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let key = new_pubkey();
    db.add_team_member("Alice", &key, Role::Administrator)
        .expect("First insert should succeed");
    let result = db.add_team_member("Alice2", &key, Role::Signer);
    assert!(result.is_err(), "Duplicate pubkey should be rejected by UNIQUE constraint");
}

#[test]
fn test_db_signature_count() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let now = Utc::now();

    assert_eq!(db.signature_count().unwrap(), 0);

    db.log_signing_event("e1", "pub", 1, now).unwrap();
    assert_eq!(db.signature_count().unwrap(), 1);

    db.log_signing_event("e2", "pub", 1, now + chrono::Duration::seconds(1)).unwrap();
    assert_eq!(db.signature_count().unwrap(), 2);
}

#[test]
fn test_find_member_by_pubkey() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let alice = new_pubkey();
    db.add_team_member("Alice", &alice, Role::User).unwrap();

    let found = db.find_member_by_pubkey(&alice).unwrap().expect("Alice is registered");
    assert_eq!(found.name, "Alice");
    assert_eq!(found.role(), Some(Role::User));

    assert!(db.find_member_by_pubkey(&new_pubkey()).unwrap().is_none(), "an unregistered key is a clear miss");
}

#[test]
fn test_administrator_count() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    assert_eq!(db.administrator_count().unwrap(), 0);
    db.add_team_member("Alice", &new_pubkey(), Role::Administrator).unwrap();
    db.add_team_member("Bob", &new_pubkey(), Role::User).unwrap();
    db.add_team_member("Carol", &new_pubkey(), Role::Administrator).unwrap();
    assert_eq!(db.administrator_count().unwrap(), 2);
}

#[test]
fn test_seed_administrator_adds_when_there_is_none() {
    let db = Database::new(":memory:").unwrap();
    let key = new_pubkey();
    db.add_team_member("Bob", &new_pubkey(), Role::User).unwrap();

    assert_eq!(db.seed_administrator(&key).unwrap(), SeedOutcome::Added);
    let seeded = db.find_member_by_pubkey(&key).unwrap().unwrap();
    assert_eq!(seeded.role(), Some(Role::Administrator));
    assert_eq!(seeded.name, "Administrator (bootstrap)");

    // A second start changes nothing.
    assert_eq!(db.seed_administrator(&key).unwrap(), SeedOutcome::AdministratorExists);
    assert_eq!(db.administrator_count().unwrap(), 1);
}

#[test]
fn test_seed_administrator_promotes_an_existing_member() {
    let db = Database::new(":memory:").unwrap();
    let key = new_pubkey();
    db.add_team_member("Alice", &key, Role::Signer).unwrap();

    assert_eq!(db.seed_administrator(&key).unwrap(), SeedOutcome::Promoted);
    let member = db.find_member_by_pubkey(&key).unwrap().unwrap();
    assert_eq!(member.role(), Some(Role::Administrator));
    assert_eq!(member.name, "Alice", "promotion keeps the member's name");
}

#[test]
fn test_seed_administrator_never_acts_while_an_administrator_exists() {
    let db = Database::new(":memory:").unwrap();
    db.add_team_member("Alice", &new_pubkey(), Role::Administrator).unwrap();
    let removed_or_other = new_pubkey();

    assert_eq!(db.seed_administrator(&removed_or_other).unwrap(), SeedOutcome::AdministratorExists);
    assert!(db.find_member_by_pubkey(&removed_or_other).unwrap().is_none(), "never re-adds a key");
}
