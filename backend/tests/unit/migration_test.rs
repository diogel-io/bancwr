// Versioned migrations (#24): a database written before #24 opens with its roles renamed and its
// pubkeys canonicalised, and opening it again changes nothing. Migration 7 (#77) maps the roles to
// administrator, signer and #77's third role, and migration 15 (#143) makes that third role signer.
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
    // viewer became user (migration 1), then signer (migration 7, #77).
    let bob_row = db.find_member_by_pubkey(&bob.to_hex()).unwrap().expect("Bob, lower-cased");
    assert_eq!(bob_row.role(), Some(Role::Signer));
    let carol_row = db.get_team_members().unwrap().into_iter().find(|m| m.name == "Carol").unwrap();
    // A pre-#24 signer became #77's third role (migration 7), then a signer again (migration 15).
    assert_eq!(carol_row.role(), Some(Role::Signer));
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

/// A database as it was before diogel-io/workspace#38: at version 4, `signing_logs` without the
/// member columns, holding `rows` of (client pubkey, timestamp), and `connections` of
/// (id, client pubkey, member, connected_at, revoked_at).
fn database_before_38(connections: &[(&str, &str, &str, &str, Option<&str>)], rows: &[(&str, &str)]) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "ALTER TABLE signing_logs DROP COLUMN member_pubkey;
         ALTER TABLE signing_logs DROP COLUMN connection_id;
         PRAGMA user_version = 4;",
    )
    .unwrap();
    for (id, client, member, connected_at, revoked_at) in connections {
        conn.execute(
            "INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at)
             VALUES (?1, ?1, ?2, ?2, 'x', 'sign_event:1', ?3, ?3)",
            params![format!("token-{}", id), member, connected_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at, revoked_at)
             VALUES (?1, ?2, ?3, ?4, 'sign_event:1', ?5, ?6)",
            params![id, client, format!("token-{}", id), member, connected_at, revoked_at],
        )
        .unwrap();
    }
    for (i, (client, timestamp)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO signing_logs (id, event_id, pubkey, event_kind, timestamp) VALUES (?1, ?2, ?3, 1, ?4)",
            params![format!("00000000-0000-4000-8000-00000000000{}", i), format!("event-{}", i), client, timestamp],
        )
        .unwrap();
    }
    file
}

#[test]
fn a_database_from_before_38_attributes_its_log_to_the_connection_active_at_each_signature() {
    let client = Keys::generate().public_key().to_hex();
    let alice = Keys::generate().public_key().to_hex();
    let bob = Keys::generate().public_key().to_hex();
    // The same client key connected for Alice, was revoked, then connected for Bob.
    let file = database_before_38(
        &[
            ("c-alice", &client, &alice, "2026-01-01T00:00:00+00:00", Some("2026-01-02T00:00:00.500+00:00")),
            ("c-bob", &client, &bob, "2026-01-03T00:00:00+00:00", None),
        ],
        &[
            (&client, "2026-01-01T12:00:00.123456789+00:00"), // Alice's connection
            (&client, "2026-01-04T00:00:00+00:00"),           // Bob's
            (&client, "2026-01-02T12:00:00+00:00"),           // between the two: neither
            (&Keys::generate().public_key().to_hex(), "2026-01-01T12:00:00+00:00"), // an unknown client
        ],
    );

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);

    let by_event = |event: &str| db.get_recent_logs(10).unwrap().into_iter().find(|l| l.event_id == event).unwrap();
    let alices = by_event("event-0");
    assert_eq!(alices.member_pubkey.as_deref(), Some(alice.as_str()));
    assert_eq!(alices.connection_id.as_deref(), Some("c-alice"));
    let bobs = by_event("event-1");
    assert_eq!(bobs.member_pubkey.as_deref(), Some(bob.as_str()));
    assert_eq!(bobs.connection_id.as_deref(), Some("c-bob"));
    for unattributed in ["event-2", "event-3"] {
        let log = by_event(unattributed);
        assert_eq!((log.member_pubkey, log.connection_id), (None, None), "{}", unattributed);
    }
}

#[test]
fn a_database_from_before_78_gains_the_bunker_relays_table() {
    // As master left it before #78: at version 5, without `bunker_relays`.
    let file = NamedTempFile::new().unwrap();
    {
        drop(Database::new(file.path().to_str().unwrap()).unwrap());
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch("DROP TABLE bunker_relays; PRAGMA user_version = 5;").unwrap();
    }
    assert_eq!(user_version(&file), 5);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert!(db.list_bunker_relays().unwrap().is_empty());
}

#[test]
fn rerunning_the_bunker_relays_migration_keeps_the_stored_list() {
    // A database whose version was rewound past #78 re-runs migration 6: it must neither fail on
    // the existing table nor empty it.
    let file = NamedTempFile::new().unwrap();
    {
        let db = Database::new(file.path().to_str().unwrap()).unwrap();
        db.replace_bunker_relays(&["wss://a.example".to_string()], "admin", chrono::Utc::now()).unwrap();
        drop(db);
        Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 5;").unwrap();
    }

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(db.list_bunker_relays().unwrap().iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), vec!["wss://a.example"]);
}

// Migration 7 (#77): administrator, user and signer become administrator, signer and viewer. Every
// database opened here also runs migration 15 (#143), so each viewer ends a signer: what these tests
// still see of migration 7 is its order and its revocations.

/// A database as #24's role model left it: fully migrated to version 6, holding `members` of
/// (name, hex pubkey, role) and, for each member named in `connected`, an active NIP-46
/// connection (and its used token) plus an unused token.
fn database_before_77(members: &[(&str, &str, &str)], connected: &[&str]) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch("DROP TABLE role_migrations; PRAGMA user_version = 6;").unwrap();
    for (i, (name, pubkey, role)) in members.iter().enumerate() {
        conn.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00+00:00')",
            params![format!("00000000-0000-4000-8000-00000000000{}", i), name, pubkey, role],
        )
        .unwrap();
        if connected.contains(name) {
            for (token, used) in [(format!("used-{}", name), "'2026-01-02T00:00:00+00:00'"), (format!("unused-{}", name), "NULL")] {
                conn.execute(
                    &format!(
                        "INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at)
                         VALUES (?1, ?1, ?2, '', 'x', 'sign_event:1', '2026-01-01T00:00:00+00:00', '2099-01-01T00:00:00+00:00', {})",
                        used
                    ),
                    params![token, pubkey],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at)
                 VALUES (?1, ?2, ?3, ?4, 'sign_event:1', '2026-01-02T00:00:00+00:00')",
                params![format!("conn-{}", name), format!("client-{}", name), format!("used-{}", name), pubkey],
            )
            .unwrap();
        }
    }
    file
}

fn role_of(db: &Database, pubkey: &str) -> String {
    db.find_member_by_pubkey(pubkey).unwrap().unwrap().role
}

#[test]
fn migrations_7_and_15_map_every_old_role_to_one_of_the_two() {
    let (admin, user, signer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_77(&[("Ada", &admin, "administrator"), ("Uma", &user, "user"), ("Sid", &signer, "signer")], &[]);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(role_of(&db, &admin), "administrator");
    assert_eq!(role_of(&db, &user), "signer");
    // A signer from before #77 was mapped to viewer, and is a signer again (#143).
    assert_eq!(role_of(&db, &signer), "signer");
    assert!(db.get_team_members().unwrap().iter().all(|m| m.role().is_some()), "every stored role is one of the two");
}

#[test]
fn migration_7_revoked_what_its_viewers_held_and_they_return_as_signers_holding_nothing() {
    let (admin, user, signer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_77(
        &[("Ada", &admin, "administrator"), ("Uma", &user, "user"), ("Sid", &signer, "signer")],
        &["Ada", "Uma", "Sid"],
    );

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    let sids = &db.list_nip46_connections(Some(&signer)).unwrap()[0];
    assert!(sids.revoked_at.is_some());
    assert_eq!(sids.revoked_reason.as_deref(), Some("role_changed"));
    assert!(db.active_nip46_connection("client-Sid").unwrap().is_none());
    let unused = db.list_nip46_tokens().unwrap().into_iter().find(|t| t.id == "unused-Sid").unwrap();
    assert!(unused.revoked_at.is_some(), "a viewer's unused token is revoked too");
    // Now a signer (#143), with no connections until a token is issued.
    assert_eq!(role_of(&db, &signer), "signer");
    assert!(db.list_nip46_connections(Some(&signer)).unwrap().iter().all(|c| c.revoked_at.is_some()));

    // The administrator and the new signer keep theirs.
    for (who, client) in [(&admin, "client-Ada"), (&user, "client-Uma")] {
        assert!(db.list_nip46_connections(Some(who)).unwrap()[0].revoked_at.is_none());
        assert!(db.active_nip46_connection(client).unwrap().is_some());
    }
    assert!(db.list_nip46_tokens().unwrap().into_iter().find(|t| t.id == "unused-Uma").unwrap().revoked_at.is_none());
    assert_eq!(db.active_nip46_connection_count().unwrap(), 2);
}

#[test]
fn rerunning_migration_7_does_not_map_the_roles_again() {
    // A database whose version was rewound past #77 re-runs migration 7. Mapping again would turn
    // every signer it made into a viewer, and revoke their connections.
    let (user, signer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_77(&[("Uma", &user, "user"), ("Sid", &signer, "signer")], &["Uma"]);
    let path = file.path().to_str().unwrap().to_string();
    drop(Database::new(&path).unwrap());
    Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 5;").unwrap();

    let db = Database::new(&path).unwrap();

    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(role_of(&db, &user), "signer");
    assert_eq!(role_of(&db, &signer), "signer");
    assert!(db.active_nip46_connection("client-Uma").unwrap().is_some(), "the signer keeps their connection");
}

#[test]
fn a_new_database_records_migration_7_so_a_rewind_keeps_its_signers() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap().to_string();
    let signer = Keys::generate().public_key().to_hex();
    {
        let db = Database::new(&path).unwrap();
        db.add_team_member("Sid", &signer, Role::Signer).unwrap();
    }
    Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 6;").unwrap();

    let db = Database::new(&path).unwrap();
    assert_eq!(role_of(&db, &signer), "signer");
}

// Migration 15 (#143): two roles. Every viewer becomes a signer.

/// A fully migrated database rewound to version 14, holding `members` of (name, hex pubkey, role),
/// and for each member named in `connected` an active NIP-46 connection.
fn database_before_143(members: &[(&str, &str, &str)], connected: &[&str]) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch("PRAGMA user_version = 14;").unwrap();
    for (i, (name, pubkey, role)) in members.iter().enumerate() {
        conn.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00+00:00')",
            params![format!("00000000-0000-4000-8000-00000000000{}", i), name, pubkey, role],
        )
        .unwrap();
        if connected.contains(name) {
            conn.execute(
                "INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at)
                 VALUES (?1, ?1, ?2, '', 'x', 'sign_event:1', '2026-01-01T00:00:00+00:00', '2099-01-01T00:00:00+00:00', '2026-01-02T00:00:00+00:00')",
                params![format!("used-{}", name), pubkey],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at)
                 VALUES (?1, ?2, ?3, ?4, 'sign_event:1', '2026-01-02T00:00:00+00:00')",
                params![format!("conn-{}", name), format!("client-{}", name), format!("used-{}", name), pubkey],
            )
            .unwrap();
        }
    }
    file
}

#[test]
fn migration_15_makes_every_viewer_a_signer_and_leaves_the_rest() {
    let keys: Vec<String> = (0..5).map(|_| Keys::generate().public_key().to_hex()).collect();
    let file = database_before_143(
        &[("Ada", &keys[0], "administrator"), ("Sid", &keys[1], "signer"), ("Vic", &keys[2], "viewer"), ("Val", &keys[3], "viewer"), ("Oscar", &keys[4], "owner")],
        &["Ada", "Sid"],
    );

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(role_of(&db, &keys[0]), "administrator");
    assert_eq!(role_of(&db, &keys[1]), "signer");
    assert_eq!(role_of(&db, &keys[2]), "signer");
    assert_eq!(role_of(&db, &keys[3]), "signer");
    assert_eq!(role_of(&db, &keys[4]), "owner", "a value it does not know is left, and grants nothing");
    // Nothing is revoked.
    assert_eq!(db.active_nip46_connection_count().unwrap(), 2);
    // A former viewer holds no connections until a token is issued.
    assert!(db.list_nip46_connections(Some(&keys[2])).unwrap().is_empty());
}

#[test]
fn rerunning_migration_15_changes_nothing() {
    let (admin, viewer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_143(&[("Ada", &admin, "administrator"), ("Vic", &viewer, "viewer")], &["Ada"]);
    let path = file.path().to_str().unwrap().to_string();
    drop(Database::new(&path).unwrap());

    // Rewound to 14, and to before #77: migration 7 skips its mapping, and 15 finds no viewers.
    for version in [14, 6] {
        Connection::open(&path).unwrap().execute_batch(&format!("PRAGMA user_version = {};", version)).unwrap();
        let db = Database::new(&path).unwrap();
        assert_eq!(user_version(&file), LATEST_VERSION as i64);
        assert_eq!(role_of(&db, &admin), "administrator");
        assert_eq!(role_of(&db, &viewer), "signer");
        assert!(db.active_nip46_connection("client-Ada").unwrap().is_some(), "the administrator keeps their connection");
        assert_eq!(db.get_team_members().unwrap().len(), 2);
    }
}

// Migrations 8 and 9 (#96): audit events, backfilled from tokens and connections, and signing log
// timestamps made fixed-width.

/// A database as #95 left it: at version 7, without `audit_events`, with a token issued by an
/// administrator and revoked, a used token whose connection was revoked by its member, and two
/// signatures in the old timestamp format.
fn database_before_96() -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "DROP TABLE audit_events; PRAGMA user_version = 7;
         INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES
           ('00000000-0000-4000-8000-000000000001', 'Ann', 'aa00000000000000000000000000000000000000000000000000000000000001', 'administrator', '2026-01-01T00:00:00+00:00');
         INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at, revoked_at) VALUES
           ('t-revoked', 'h1', 'aa00000000000000000000000000000000000000000000000000000000000001',
            'aa00000000000000000000000000000000000000000000000000000000000001', 'x', 'sign_event:1',
            '2026-01-02T00:00:00.5+00:00', '2026-01-03T00:00:00+00:00', NULL, '2026-01-02T01:00:00+00:00'),
           ('t-used', 'h2', 'aa00000000000000000000000000000000000000000000000000000000000001', '', 'y', 'sign_event:7',
            '2026-01-04T00:00:00+00:00', '2026-01-05T00:00:00+00:00', '2026-01-04T00:10:00+00:00', NULL);
         INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at, revoked_at, revoked_reason, revoked_by) VALUES
           ('c-1', 'cc00000000000000000000000000000000000000000000000000000000000003', 't-used',
            'aa00000000000000000000000000000000000000000000000000000000000001', 'sign_event:7',
            '2026-01-04T00:10:00+00:00', '2026-01-04T02:00:00.123+00:00', 'revoked',
            'aa00000000000000000000000000000000000000000000000000000000000001');
         INSERT INTO signing_logs (id, event_id, pubkey, event_kind, timestamp) VALUES
           ('00000000-0000-4000-8000-0000000000a1', 'e1', 'cc', 1, '2026-01-04T00:20:00+00:00'),
           ('00000000-0000-4000-8000-0000000000a2', 'e2', 'cc', 1, '2026-01-04T00:20:00.25+01:00'),
           ('00000000-0000-4000-8000-0000000000a3', 'e3', 'cc', 1, 'not a time');",
    )
    .unwrap();
    file
}

#[test]
fn migrating_to_96_backfills_audit_events_from_tokens_and_connections() {
    let file = database_before_96();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);

    let events = db.audit_events().unwrap();
    let summary: Vec<(String, String, Option<String>, Option<String>)> =
        events.iter().map(|e| (e.time.clone(), e.action.clone(), e.actor_pubkey.clone(), e.detail.clone())).collect();
    let admin = Some("aa00000000000000000000000000000000000000000000000000000000000001".to_string());
    assert_eq!(
        summary,
        vec![
            ("2026-01-02T00:00:00.500000Z".into(), "token_issued".into(), admin.clone(), Some("perms=sign_event:1;expires_at=2026-01-03T00:00:00.000000Z".into())),
            ("2026-01-02T01:00:00.000000Z".into(), "token_revoked".into(), None, Some("reason=unknown".into())),
            ("2026-01-04T00:00:00.000000Z".into(), "token_issued".into(), None, Some("perms=sign_event:7;expires_at=2026-01-05T00:00:00.000000Z".into())),
            ("2026-01-04T00:10:00.000000Z".into(), "connection_created".into(), None, Some("perms=sign_event:7".into())),
            ("2026-01-04T02:00:00.123000Z".into(), "connection_revoked".into(), admin.clone(), Some("reason=revoked".into())),
        ]
    );
    assert!(events.iter().all(|e| e.member_name.as_deref() == Some("Ann")));
    let created = &events[3];
    assert_eq!((created.connection_id.as_deref(), created.token_id.as_deref()), (Some("c-1"), Some("t-used")));
}

#[test]
fn migrating_to_96_makes_signing_log_timestamps_fixed_width_and_keeps_an_unreadable_one() {
    let file = database_before_96();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    let stamps: Vec<(String, String)> = conn
        .prepare("SELECT event_id, timestamp FROM signing_logs ORDER BY event_id").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert_eq!(
        stamps,
        vec![
            ("e1".to_string(), "2026-01-04T00:20:00.000000Z".to_string()),
            // +01:00 becomes UTC, so it now sorts before e1, as it happened.
            ("e2".to_string(), "2026-01-03T23:20:00.250000Z".to_string()),
            ("e3".to_string(), "not a time".to_string()),
        ]
    );
}

#[test]
fn rerunning_the_96_migrations_duplicates_nothing() {
    let file = database_before_96();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    let count = db.audit_event_count().unwrap();
    db.add_team_member("Later", "bb00000000000000000000000000000000000000000000000000000000000002", Role::Signer).unwrap();
    drop(db);
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 7;").unwrap();

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(db.audit_event_count().unwrap(), count + 1, "the backfill once, and the later event kept");
}

/// A database as #97 left it (version 9): no connection lifetimes, one token and the connection it
/// made, still active.
fn database_before_98() -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "ALTER TABLE nip46_tokens DROP COLUMN connection_lifetime_hours;
         ALTER TABLE nip46_connections DROP COLUMN expires_at;
         PRAGMA user_version = 9;
         INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES
           ('00000000-0000-4000-8000-000000000001', 'Ann', 'aa00000000000000000000000000000000000000000000000000000000000001', 'administrator', '2026-01-01T00:00:00+00:00');
         INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at, revoked_at) VALUES
           ('t-used', 'h1', 'aa00000000000000000000000000000000000000000000000000000000000001', '', 'x', 'sign_event:1',
            '2026-01-04T00:00:00+00:00', '2026-01-05T00:00:00+00:00', '2026-01-04T00:10:00+00:00', NULL);
         INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at) VALUES
           ('c-1', 'cc00000000000000000000000000000000000000000000000000000000000003', 't-used',
            'aa00000000000000000000000000000000000000000000000000000000000001', 'sign_event:1', '2026-01-04T00:10:00+00:00');",
    )
    .unwrap();
    file
}

#[test]
fn migrating_to_98_adds_connection_lifetimes_and_leaves_existing_connections_unlimited() {
    let file = database_before_98();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);

    let tokens = db.list_nip46_tokens().unwrap();
    assert_eq!(tokens[0].connection_lifetime_hours, None);
    let client = "cc00000000000000000000000000000000000000000000000000000000000003";
    let connection = db.active_nip46_connection(client).unwrap().expect("still active");
    assert_eq!(connection.expires_at, None);
    // Unlimited, however far ahead.
    let later = chrono::Utc::now() + chrono::Duration::days(3650);
    assert!(db.active_nip46_connection_at(client, later).unwrap().is_some());
    assert!(db.expire_nip46_connections(later).unwrap().is_empty());
}

#[test]
fn rerunning_the_98_migration_changes_nothing() {
    let file = database_before_98();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 9;").unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(db.list_nip46_connections(None).unwrap().len(), 1);
}

/// A database as #98 left it (version 10): no rate limits, one token and the connection it made.
fn database_before_101() -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "ALTER TABLE nip46_tokens DROP COLUMN rate_per_minute;
         ALTER TABLE nip46_tokens DROP COLUMN rate_burst;
         ALTER TABLE nip46_connections DROP COLUMN rate_per_minute;
         ALTER TABLE nip46_connections DROP COLUMN rate_burst;
         PRAGMA user_version = 10;
         INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES
           ('00000000-0000-4000-8000-000000000001', 'Ann', 'aa00000000000000000000000000000000000000000000000000000000000001', 'administrator', '2026-01-01T00:00:00+00:00');
         INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at, revoked_at) VALUES
           ('t-used', 'h1', 'aa00000000000000000000000000000000000000000000000000000000000001', '', 'x', 'sign_event:1',
            '2026-01-04T00:00:00+00:00', '2026-01-05T00:00:00+00:00', '2026-01-04T00:10:00+00:00', NULL);
         INSERT INTO nip46_connections (id, client_pubkey, token_id, for_pubkey, perms, connected_at) VALUES
           ('c-1', 'cc00000000000000000000000000000000000000000000000000000000000003', 't-used',
            'aa00000000000000000000000000000000000000000000000000000000000001', 'sign_event:1', '2026-01-04T00:10:00+00:00');",
    )
    .unwrap();
    file
}

fn columns(file: &NamedTempFile, table: &str) -> Vec<String> {
    let conn = Connection::open(file.path()).unwrap();
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{}')", table)).unwrap();
    stmt.query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn migrating_to_101_adds_rate_limits_and_existing_connections_get_the_default() {
    let file = database_before_101();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    for table in ["nip46_tokens", "nip46_connections"] {
        let names = columns(&file, table);
        assert!(names.iter().any(|c| c == "rate_per_minute") && names.iter().any(|c| c == "rate_burst"), "{}: {:?}", table, names);
    }

    let tokens = db.list_nip46_tokens().unwrap();
    assert_eq!(tokens[0].rate_limit, None);
    let connection = db.active_nip46_connection("cc00000000000000000000000000000000000000000000000000000000000003").unwrap().expect("still active");
    assert_eq!(connection.rate_limit, None);
    assert_eq!(connection.effective_rate_limit(), bunker::rate_limit::RateLimit::DEFAULT);
}

#[test]
fn rerunning_the_101_migration_changes_nothing() {
    let file = database_before_101();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 10;").unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(db.list_nip46_connections(None).unwrap().len(), 1);
}

#[test]
fn migrating_to_102_adds_approval_kinds_and_existing_connections_need_none() {
    // As #101 left it: version 11, with one token and its connection, and no approval columns.
    let file = database_before_101();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "ALTER TABLE nip46_tokens DROP COLUMN approval_perms;
         ALTER TABLE nip46_connections DROP COLUMN approval_perms;
         PRAGMA user_version = 11;",
    )
    .unwrap();
    drop(conn);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    for table in ["nip46_tokens", "nip46_connections"] {
        assert!(columns(&file, table).iter().any(|c| c == "approval_perms"), "{}", table);
    }
    assert!(db.list_nip46_tokens().unwrap()[0].approval_perms.is_empty());
    let connection = db.active_nip46_connection("cc00000000000000000000000000000000000000000000000000000000000003").unwrap().expect("still active");
    assert!(connection.approval_perms.is_empty());
    assert!(!connection.needs_approval(1));

    // Re-running it changes nothing.
    drop(db);
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 11;").unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(db.list_nip46_connections(None).unwrap().len(), 1);
}

#[test]
fn migrating_to_103_leaves_a_single_key_bunker_as_it_was() {
    // As #102 left it: version 12, one token and its connection, no identities.
    let file = database_before_101();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    let conn = Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "DROP TABLE identity_access;
         DROP TABLE identities;
         ALTER TABLE nip46_tokens DROP COLUMN identity_pubkey;
         ALTER TABLE nip46_connections DROP COLUMN identity_pubkey;
         ALTER TABLE signing_logs DROP COLUMN identity_pubkey;
         ALTER TABLE audit_events DROP COLUMN identity_pubkey;
         PRAGMA user_version = 12;",
    )
    .unwrap();
    drop(conn);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    for table in ["nip46_tokens", "nip46_connections", "signing_logs", "audit_events"] {
        assert!(columns(&file, table).iter().any(|c| c == "identity_pubkey"), "{}", table);
    }
    assert!(db.list_identities().unwrap().is_empty(), "the primary is recorded at start, not by the migration");
    let client = "cc00000000000000000000000000000000000000000000000000000000000003";
    let connection = db.active_nip46_connection(client).unwrap().expect("still active");
    assert_eq!(connection.identity_pubkey, None);
    assert_eq!(db.list_nip46_tokens().unwrap()[0].identity_pubkey, None);
    assert!(db.may_use_identity(&connection.for_pubkey, None).unwrap());

    // Re-running it changes nothing.
    drop(db);
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 12;").unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert!(db.active_nip46_connection(client).unwrap().is_some());
}

#[test]
fn migrating_to_14_adds_the_nip46_senders_table_and_keeps_connected_apps_refused_as_peers() {
    // As #103 left it: version 13, one token and its connection, no record of senders.
    let file = database_before_101();
    drop(Database::new(file.path().to_str().unwrap()).unwrap());
    Connection::open(file.path()).unwrap().execute_batch("DROP TABLE nip46_senders; PRAGMA user_version = 13;").unwrap();

    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(columns(&file, "nip46_senders"), vec!["pubkey".to_string(), "first_seen_at".to_string()]);
    let client = "cc00000000000000000000000000000000000000000000000000000000000003";
    assert!(db.is_nip46_client(client).unwrap(), "a key that connected before is still refused");
    let sender = "dd00000000000000000000000000000000000000000000000000000000000004";
    assert!(!db.is_nip46_client(sender).unwrap());
    db.record_nip46_sender(sender, chrono::Utc::now()).unwrap();

    // Re-running it keeps what it recorded.
    drop(db);
    Connection::open(file.path()).unwrap().execute_batch("PRAGMA user_version = 13;").unwrap();
    let db = Database::new(file.path().to_str().unwrap()).unwrap();
    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert!(db.is_nip46_client(sender).unwrap());
}
