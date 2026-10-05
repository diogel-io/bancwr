use bunker::db::{ClientMetadata, Database, RemoveOutcome, RoleChangeOutcome, SeedOutcome};
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

    db.log_signing_event(event_id, pubkey, None, None, kind, now).expect("Failed to log event");

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

    db.log_signing_event("event1", "pub1", None, None, 1, now).unwrap();
    db.log_signing_event("event2", "pub2", None, None, 2, now + chrono::Duration::seconds(1)).unwrap();

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

    // Remove member. A second administrator first, so Alice is not the last one (#25).
    db.add_team_member("Carol", &new_pubkey(), Role::Administrator).unwrap();
    let removed = db.remove_team_member(id).expect("Failed to remove member");
    assert_eq!(removed, RemoveOutcome::Removed);
    let members = db.get_team_members().expect("Failed to get members");
    assert_eq!(members.len(), 1);

    // Removing it again finds nothing
    let removed_again = db.remove_team_member(id).expect("Failed to remove member");
    assert_eq!(removed_again, RemoveOutcome::NotFound);
}

#[test]
fn test_the_last_administrator_cannot_be_removed() {
    let db = Database::new(":memory:").unwrap();
    let alice = db.add_team_member("Alice", &new_pubkey(), Role::Administrator).unwrap();
    let bob = db.add_team_member("Bob", &new_pubkey(), Role::Signer).unwrap();

    assert_eq!(db.remove_team_member(alice).unwrap(), RemoveOutcome::LastAdministrator);
    assert_eq!(db.administrator_count().unwrap(), 1, "she is still there");
    // Other members are unaffected.
    assert_eq!(db.remove_team_member(bob).unwrap(), RemoveOutcome::Removed);

    // With a second administrator, either can go, but not both.
    let carol = db.add_team_member("Carol", &new_pubkey(), Role::Administrator).unwrap();
    assert_eq!(db.remove_team_member(alice).unwrap(), RemoveOutcome::Removed);
    assert_eq!(db.remove_team_member(carol).unwrap(), RemoveOutcome::LastAdministrator);
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
        db.log_signing_event("evt_persist", "pubkey_persist", None, None, 1, Utc::now())
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
    db.log_signing_event("oldest", "pub", None, None, 1, base).unwrap();
    db.log_signing_event("newest", "pub", None, None, 1, base + chrono::Duration::seconds(10)).unwrap();
    db.log_signing_event("middle", "pub", None, None, 1, base + chrono::Duration::seconds(5)).unwrap();

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

    db.log_signing_event("e1", "pub", None, None, 1, now).unwrap();
    assert_eq!(db.signature_count().unwrap(), 1);

    db.log_signing_event("e2", "pub", None, None, 1, now + chrono::Duration::seconds(1)).unwrap();
    assert_eq!(db.signature_count().unwrap(), 2);
}

#[test]
fn test_find_member_by_pubkey() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    let alice = new_pubkey();
    db.add_team_member("Alice", &alice, Role::Signer).unwrap();

    let found = db.find_member_by_pubkey(&alice).unwrap().expect("Alice is registered");
    assert_eq!(found.name, "Alice");
    assert_eq!(found.role(), Some(Role::Signer));

    assert!(db.find_member_by_pubkey(&new_pubkey()).unwrap().is_none(), "an unregistered key is a clear miss");
}

#[test]
fn test_administrator_count() {
    let db = Database::new(":memory:").expect("Failed to create in-memory database");
    assert_eq!(db.administrator_count().unwrap(), 0);
    db.add_team_member("Alice", &new_pubkey(), Role::Administrator).unwrap();
    db.add_team_member("Bob", &new_pubkey(), Role::Signer).unwrap();
    db.add_team_member("Carol", &new_pubkey(), Role::Administrator).unwrap();
    assert_eq!(db.administrator_count().unwrap(), 2);
}

#[test]
fn test_seed_administrator_adds_when_there_is_none() {
    let db = Database::new(":memory:").unwrap();
    let key = new_pubkey();
    db.add_team_member("Bob", &new_pubkey(), Role::Signer).unwrap();

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

#[test]
fn a_log_entry_carries_its_member_and_names_them_while_they_are_in_the_vault() {
    // diogel-io/workspace#38
    let db = Database::new(":memory:").unwrap();
    let member = new_pubkey();
    let id = db.add_team_member("Alice", &member, Role::Signer).unwrap();
    let client = new_pubkey();
    db.log_signing_event("e1", &client, Some(&member), Some("conn-1"), 1, Utc::now()).unwrap();

    let log = &db.get_recent_logs(10).unwrap()[0];
    assert_eq!(log.pubkey, client);
    assert_eq!(log.member_pubkey.as_deref(), Some(member.as_str()));
    assert_eq!(log.member_name.as_deref(), Some("Alice"));
    assert_eq!(log.connection_id.as_deref(), Some("conn-1"));

    // Removed: the entry keeps the key, and no longer has a name.
    db.remove_team_member(id).unwrap();
    let log = &db.get_recent_logs(10).unwrap()[0];
    assert_eq!(log.member_pubkey.as_deref(), Some(member.as_str()));
    assert_eq!(log.member_name, None);
}

#[test]
fn an_unattributed_log_entry_has_no_member() {
    let db = Database::new(":memory:").unwrap();
    db.log_signing_event("e1", &new_pubkey(), None, None, 1, Utc::now()).unwrap();
    let log = &db.get_recent_logs(10).unwrap()[0];
    assert_eq!((log.member_pubkey.as_ref(), log.member_name.as_ref(), log.connection_id.as_ref()), (None, None, None));
}

// The bunker's own relays (#78): saved whole, in the administrator's order.
#[test]
fn bunker_relays_are_replaced_whole_and_listed_in_order() {
    let db = Database::new(":memory:").unwrap();
    assert!(db.list_bunker_relays().unwrap().is_empty());

    let first = Utc::now() - chrono::Duration::hours(1);
    let urls = |list: &[&str]| list.iter().map(|u| u.to_string()).collect::<Vec<_>>();
    db.replace_bunker_relays(&urls(&["wss://c.example", "wss://a.example", "wss://b.example"]), "alice", first).unwrap();
    let listed = db.list_bunker_relays().unwrap();
    assert_eq!(listed.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), vec!["wss://c.example", "wss://a.example", "wss://b.example"]);
    assert_eq!(listed.iter().map(|r| r.position).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(listed.iter().all(|r| r.added_by == "alice"));

    // Reordered, one dropped, one added: a relay kept keeps who added it and when.
    let later = Utc::now();
    db.replace_bunker_relays(&urls(&["wss://b.example", "wss://d.example", "wss://c.example"]), "bob", later).unwrap();
    let listed = db.list_bunker_relays().unwrap();
    assert_eq!(listed.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), vec!["wss://b.example", "wss://d.example", "wss://c.example"]);
    let by_url = |url: &str| listed.iter().find(|r| r.url == url).unwrap().clone();
    assert_eq!(by_url("wss://b.example").added_by, "alice");
    assert_eq!(by_url("wss://b.example").added_at.timestamp(), first.timestamp());
    assert_eq!(by_url("wss://d.example").added_by, "bob");

    db.replace_bunker_relays(&[], "bob", Utc::now()).unwrap();
    assert!(db.list_bunker_relays().unwrap().is_empty());
}

#[test]
fn a_failed_bunker_relays_save_changes_nothing() {
    let db = Database::new(":memory:").unwrap();
    db.replace_bunker_relays(&["wss://a.example".to_string()], "alice", Utc::now()).unwrap();

    // A duplicate breaks the primary key part-way through: the transaction keeps the old list.
    let duplicate = vec!["wss://b.example".to_string(), "wss://b.example".to_string()];
    assert!(db.replace_bunker_relays(&duplicate, "alice", Utc::now()).is_err());
    assert_eq!(db.list_bunker_relays().unwrap().iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), vec!["wss://a.example"]);
}

// Roles (#77): a member who becomes a viewer cannot sign.

/// A connection for `member`, made through a token as the API makes one; returns its client key.
fn connect(db: &Database, member: &str) -> String {
    let now = Utc::now();
    let secret = format!("secret-{}", new_pubkey());
    db.create_nip46_token(&bunker::nip46::hash_secret(&secret), member, "", "x", &["sign_event:1".to_string()], now, now + chrono::Duration::hours(1))
        .unwrap();
    let client = new_pubkey();
    db.redeem_nip46_token(&bunker::nip46::hash_secret(&secret), &client, None, &ClientMetadata::default(), now).unwrap().unwrap();
    client
}

#[test]
fn changing_a_member_to_viewer_revokes_their_connections_and_tokens() {
    let db = Database::new(":memory:").unwrap();
    db.add_team_member("Admin", &new_pubkey(), Role::Administrator).unwrap();
    let alice = new_pubkey();
    let bob = new_pubkey();
    db.add_team_member("Alice", &alice, Role::Signer).unwrap();
    db.add_team_member("Bob", &bob, Role::Signer).unwrap();
    let alice_client = connect(&db, &alice);
    let bob_client = connect(&db, &bob);
    let now = Utc::now();
    db.create_nip46_token("unused", &alice, "", "x", &["sign_event:1".to_string()], now, now + chrono::Duration::hours(1)).unwrap();

    assert_eq!(db.change_member_role(&alice, Role::Viewer).unwrap(), RoleChangeOutcome::Changed);

    assert_eq!(db.find_member_by_pubkey(&alice).unwrap().unwrap().role(), Some(Role::Viewer));
    assert!(db.active_nip46_connection(&alice_client).unwrap().is_none());
    let hers = &db.list_nip46_connections(Some(&alice)).unwrap()[0];
    assert_eq!(hers.revoked_reason.as_deref(), Some("role_changed"));
    assert!(db.list_nip46_tokens().unwrap().iter().filter(|t| t.for_pubkey == alice).all(|t| t.revoked_at.is_some() || t.used_at.is_some()));
    // Bob is untouched.
    assert!(db.active_nip46_connection(&bob_client).unwrap().is_some());
    assert_eq!(db.active_nip46_connection_count().unwrap(), 1);

    // Back to signer: nothing is restored, a new token is needed.
    assert_eq!(db.change_member_role(&alice, Role::Signer).unwrap(), RoleChangeOutcome::Changed);
    assert!(db.active_nip46_connection(&alice_client).unwrap().is_none());
}

#[test]
fn changing_to_a_role_that_signs_keeps_connections() {
    let db = Database::new(":memory:").unwrap();
    db.add_team_member("Admin", &new_pubkey(), Role::Administrator).unwrap();
    let alice = new_pubkey();
    db.add_team_member("Alice", &alice, Role::Signer).unwrap();
    let client = connect(&db, &alice);
    assert_eq!(db.change_member_role(&alice, Role::Administrator).unwrap(), RoleChangeOutcome::Changed);
    assert!(db.active_nip46_connection(&client).unwrap().is_some());
}

#[test]
fn the_last_administrator_is_never_demoted_and_an_unknown_key_is_not_found() {
    let db = Database::new(":memory:").unwrap();
    let alice = new_pubkey();
    db.add_team_member("Alice", &alice, Role::Administrator).unwrap();
    assert_eq!(db.change_member_role(&alice, Role::Viewer).unwrap(), RoleChangeOutcome::LastAdministrator);
    assert_eq!(db.find_member_by_pubkey(&alice).unwrap().unwrap().role(), Some(Role::Administrator));
    assert_eq!(db.change_member_role(&alice, Role::Administrator).unwrap(), RoleChangeOutcome::Changed, "no change is no demotion");
    assert_eq!(db.change_member_role(&new_pubkey(), Role::Signer).unwrap(), RoleChangeOutcome::NotFound);
}

#[test]
fn a_viewers_connection_signs_nothing_even_if_it_was_not_revoked() {
    // The defence behind the revocation (#77): a viewer's connection is refused as a removed
    // member's is, however the role came to be stored.
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let db = Database::new(path).unwrap();
    let alice = new_pubkey();
    db.add_team_member("Alice", &alice, Role::Signer).unwrap();
    let client = connect(&db, &alice);
    rusqlite::Connection::open(path).unwrap().execute("UPDATE team_members SET role = 'viewer'", []).unwrap();

    assert!(db.active_nip46_connection(&client).unwrap().is_none());
    assert_eq!(db.active_nip46_connection_count().unwrap(), 0);
}

#[test]
fn a_members_own_logs_are_only_theirs() {
    let db = Database::new(":memory:").unwrap();
    let alice = new_pubkey();
    let bob = new_pubkey();
    let now = Utc::now();
    db.log_signing_event("a1", &new_pubkey(), Some(&alice), Some("c-a"), 1, now).unwrap();
    db.log_signing_event("b1", &new_pubkey(), Some(&bob), Some("c-b"), 1, now).unwrap();
    db.log_signing_event("a2", &new_pubkey(), Some(&alice), Some("c-a"), 7, now + chrono::Duration::seconds(1)).unwrap();
    db.log_signing_event("x", &alice, None, None, 1, now).unwrap();

    let mine: Vec<String> = db.get_recent_logs_for_member(&alice, 10).unwrap().into_iter().map(|l| l.event_id).collect();
    assert_eq!(mine, vec!["a2", "a1"], "newest first; never by the client key");
    assert_eq!(db.get_recent_logs_for_member(&alice, 1).unwrap().len(), 1);
}
