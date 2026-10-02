// Versioned migrations (#24): a database written before #24 opens with its roles renamed and its
// pubkeys canonicalised, and opening it again changes nothing.
use bunker::db::Database;
use bunker::migrations::LATEST_VERSION;
use bunker::registry::Role;
use nostr::prelude::*;
use rusqlite::{params, Connection};
use tempfile::NamedTempFile;

/// The pre-#24 schema, at user_version 0, with rows only the old validation would have allowed.
fn old_database(rows: &[(&str, &str, &str, &str)]) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE team_members (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, pubkey TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL, created_at TEXT NOT NULL
        );",
    )
    .unwrap();
    for (i, (name, pubkey, role, created_at)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![format!("00000000-0000-4000-8000-00000000000{}", i), name, pubkey, role, created_at],
        )
        .unwrap();
    }
    file
}

fn user_version(file: &NamedTempFile) -> i64 {
    Connection::open(file.path()).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn renames_roles_and_canonicalises_pubkeys() {
    let alice = Keys::generate().public_key();
    let bob = Keys::generate().public_key();
    let alice_npub = alice.to_bech32().unwrap();
    let bob_upper_hex = bob.to_hex().to_uppercase();
    let file = old_database(&[
        ("Alice", &alice_npub, "admin", "2026-01-01T00:00:00+00:00"),
        ("Bob", &bob_upper_hex, "viewer", "2026-01-02T00:00:00+00:00"),
        ("Carol", &Keys::generate().public_key().to_hex(), "signer", "2026-01-03T00:00:00+00:00"),
    ]);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    let alice_row = db.find_member_by_pubkey(&alice.to_hex()).unwrap().expect("Alice, found by hex");
    assert_eq!(alice_row.role(), Some(Role::Administrator));
    let bob_row = db.find_member_by_pubkey(&bob.to_hex()).unwrap().expect("Bob, lower-cased");
    assert_eq!(bob_row.role(), Some(Role::User));
    assert_eq!(db.administrator_count().unwrap(), 1);
    assert_eq!(db.get_team_members().unwrap().len(), 3);
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
}

#[test]
fn keeps_the_oldest_of_two_encodings_of_one_key() {
    let key = Keys::generate().public_key();
    let file = old_database(&[
        ("Newer", &key.to_hex(), "signer", "2026-02-01T00:00:00+00:00"),
        ("Older", &key.to_bech32().unwrap(), "admin", "2026-01-01T00:00:00+00:00"),
    ]);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    let members = db.get_team_members().unwrap();
    assert_eq!(members.len(), 1, "the UNIQUE constraint could not see this duplicate; the migration can");
    assert_eq!(members[0].name, "Older");
    assert_eq!(members[0].pubkey, key.to_hex());
    assert_eq!(members[0].role(), Some(Role::Administrator));
}

#[test]
fn leaves_values_it_cannot_fix_and_they_grant_nothing() {
    let file = old_database(&[
        ("Mallory", "npub1bob0000000000000000000000000000000000000000000000000000000000", "signer", "2026-01-01T00:00:00+00:00"),
        ("Oscar", &Keys::generate().public_key().to_hex(), "owner", "2026-01-02T00:00:00+00:00"),
    ]);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    let members = db.get_team_members().unwrap();
    assert_eq!(members.len(), 2, "nothing is deleted for being unreadable");
    let mallory = members.iter().find(|m| m.name == "Mallory").unwrap();
    assert!(mallory.pubkey.starts_with("npub1bob"), "an invalid key is left as it was");
    let oscar = members.iter().find(|m| m.name == "Oscar").unwrap();
    assert_eq!(oscar.role, "owner");
    assert_eq!(oscar.role(), None, "an unrecognised role grants nothing");
}

#[test]
fn opening_again_applies_nothing() {
    let key = Keys::generate().public_key();
    let file = old_database(&[("Alice", &key.to_bech32().unwrap(), "admin", "2026-01-01T00:00:00+00:00")]);
    let path = file.path().to_str().unwrap().to_string();

    drop(Database::new(&path).unwrap());
    // A value only a re-run of migration 1 would change: it must stay.
    Connection::open(&path).unwrap().execute("UPDATE team_members SET role = 'admin'", []).unwrap();
    let db = Database::new(&path).unwrap();

    assert_eq!(db.get_team_members().unwrap()[0].role, "admin");
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
}

#[test]
fn a_new_database_starts_at_the_latest_version() {
    let file = NamedTempFile::new().unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(db.get_team_members().unwrap().len(), 0);
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
}

#[test]
fn a_database_from_before_53_gains_the_nip46_tables() {
    // As master left it before #53: migrated to version 2, without the NIP-46 tables.
    let file = NamedTempFile::new().unwrap();
    {
        let db = Database::new(file.path().to_str().unwrap()).unwrap();
        drop(db);
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch("DROP TABLE nip46_connections; DROP TABLE nip46_tokens; PRAGMA user_version = 2;").unwrap();
    }
    assert_eq!(user_version(&file), 2);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert!(db.list_nip46_tokens().unwrap().is_empty());
    assert!(db.list_nip46_connections(None).unwrap().is_empty());
}

#[test]
fn a_database_from_before_31_gains_revoked_by() {
    let file = NamedTempFile::new().unwrap();
    {
        drop(Database::new(file.path().to_str().unwrap()).unwrap());
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch("ALTER TABLE nip46_connections DROP COLUMN revoked_by; PRAGMA user_version = 3;").unwrap();
    }
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert!(db.list_nip46_connections(None).unwrap().is_empty());
}
