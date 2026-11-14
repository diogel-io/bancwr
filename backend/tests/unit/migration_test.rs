// Versioned migrations (#24): a database written before #24 opens with its roles renamed and its
// pubkeys canonicalised, and opening it again changes nothing. Migration 7 (#77) maps the roles to
// administrator, signer and viewer.
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
    assert_eq!(carol_row.role(), Some(Role::Viewer), "a pre-#24 signer is a viewer now (#77)");
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

// Migration 7 (#77): administrator, user and signer become administrator, signer and viewer.

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
fn migration_7_maps_signer_to_viewer_then_user_to_signer() {
    let (admin, user, signer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_77(&[("Ada", &admin, "administrator"), ("Uma", &user, "user"), ("Sid", &signer, "signer")], &[]);

    let db = Database::new(file.path().to_str().unwrap()).unwrap();

    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(role_of(&db, &admin), "administrator");
    // The order matters: user to signer first would have made Uma a viewer as well.
    assert_eq!(role_of(&db, &user), "signer");
    assert_eq!(role_of(&db, &signer), "viewer");
    assert!(db.get_team_members().unwrap().iter().all(|m| m.role().is_some()), "every stored role is one of the three");
}

#[test]
fn migration_7_revokes_the_connections_of_members_who_are_now_viewers() {
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
    // every signer it made into a viewer.
    let (user, signer) = (Keys::generate().public_key().to_hex(), Keys::generate().public_key().to_hex());
    let file = database_before_77(&[("Uma", &user, "user"), ("Sid", &signer, "signer")], &["Uma"]);
    let path = file.path().to_str().unwrap().to_string();
    drop(Database::new(&path).unwrap());
    Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 5;").unwrap();

    let db = Database::new(&path).unwrap();

    assert_eq!(user_version(&file), LATEST_VERSION as i64);
    assert_eq!(role_of(&db, &user), "signer");
    assert_eq!(role_of(&db, &signer), "viewer");
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
