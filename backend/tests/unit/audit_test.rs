// The audit trail in the database (#96): an event for every change, paging, export batches and
// retention.

use bunker::audit::{self, action, date_bounds, db_time, Cursor, Retention, RetentionRefusal, Source};
use bunker::db::{ClientMetadata, Database, RemoveOutcome};
use bunker::logs_api::{csv_field, csv_line};
use bunker::registry::Role;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};

const ADMIN: &str = "aa00000000000000000000000000000000000000000000000000000000000001";
const SIGNER: &str = "bb00000000000000000000000000000000000000000000000000000000000002";
const CLIENT: &str = "cc00000000000000000000000000000000000000000000000000000000000003";

fn db() -> Database {
    let db = Database::new(":memory:").unwrap();
    db.add_team_member_as("Admin", ADMIN, Role::Administrator, None).unwrap();
    db.add_team_member_as("Sam", SIGNER, Role::Signer, Some(ADMIN)).unwrap();
    db
}

fn actions(db: &Database) -> Vec<String> {
    db.audit_events().unwrap().into_iter().map(|e| e.action).collect()
}

fn last(db: &Database) -> audit::ExportRow {
    db.audit_events().unwrap().pop().unwrap()
}

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap() + Duration::seconds(seconds)
}

/// A token for SIGNER issued by ADMIN, redeemed by CLIENT. Returns the connection id.
fn connect(db: &Database, secret: &str) -> String {
    let now = Utc::now();
    db.create_nip46_token(secret, SIGNER, ADMIN, "Laptop", &["sign_event:1".to_string()], now, now + Duration::hours(1)).unwrap();
    db.redeem_nip46_token(secret, CLIENT, None, &ClientMetadata::default(), now).unwrap().unwrap().id
}

#[test]
fn db_time_is_fixed_width_utc_so_text_order_is_time_order() {
    let whole = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 5).unwrap();
    let fraction = whole + Duration::microseconds(500_000);
    assert_eq!(db_time(whole), "2026-06-01T12:00:05.000000Z");
    assert_eq!(db_time(fraction), "2026-06-01T12:00:05.500000Z");
    assert!(db_time(whole) < db_time(fraction));
    // A date bound and a row in what chrono's to_rfc3339 wrote before #96 disagree at midnight.
    assert!("2026-06-01T00:00:00+00:00" < db_time(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()).as_str());
}

#[test]
fn adding_a_member_is_recorded_with_who_added_them_and_their_role() {
    let db = db();
    let added = last(&db);
    assert_eq!(added.action, action::MEMBER_ADDED);
    assert_eq!(added.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(added.member_pubkey.as_deref(), Some(SIGNER));
    assert_eq!(added.member_name.as_deref(), Some("Sam"));
    assert_eq!(added.detail.as_deref(), Some("role=signer"));
    assert_eq!(added.source, Source::Audit);
}

#[test]
fn issuing_connecting_and_revoking_are_each_recorded() {
    let db = db();
    let connection = connect(&db, "hash-1");
    assert!(db.revoke_nip46_connection_as(&connection, "revoked", Some(ADMIN), None, Utc::now()).unwrap());

    let events = db.audit_events().unwrap();
    let tail: Vec<_> = events.iter().rev().take(3).rev().collect();
    assert_eq!(tail.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), [action::TOKEN_ISSUED, action::CONNECTION_CREATED, action::CONNECTION_REVOKED]);

    let (issued, created, revoked) = (tail[0], tail[1], tail[2]);
    assert_eq!(issued.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(issued.member_pubkey.as_deref(), Some(SIGNER));
    assert!(issued.detail.as_deref().unwrap().starts_with("perms=sign_event:1;expires_at="));
    assert_eq!(created.connection_id.as_deref(), Some(connection.as_str()));
    assert_eq!(created.token_id, issued.token_id);
    assert_eq!(created.client_pubkey.as_deref(), Some(CLIENT));
    assert_eq!(created.actor_pubkey, None, "the app connected itself");
    assert_eq!(revoked.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(revoked.detail.as_deref(), Some("reason=revoked"));
}

#[test]
fn a_revocation_that_changes_nothing_records_nothing() {
    let db = db();
    let before = db.audit_event_count().unwrap();
    assert!(!db.revoke_nip46_connection_as("no-such-connection", "revoked", Some(ADMIN), None, Utc::now()).unwrap());
    assert!(!db.revoke_nip46_token_as("no-such-token", Some(ADMIN), Utc::now()).unwrap());
    // Another member's connection, revoked by someone who may only revoke their own.
    let connection = connect(&db, "hash-1");
    let before_other = db.audit_event_count().unwrap();
    assert!(!db.revoke_nip46_connection_as(&connection, "revoked", Some(ADMIN), Some(ADMIN), Utc::now()).unwrap());
    assert_eq!(db.audit_event_count().unwrap(), before_other);
    assert_eq!(before + 2, before_other, "only the issue and the connect");
}

#[test]
fn a_token_revocation_names_who_and_the_member() {
    let db = db();
    let now = Utc::now();
    let token = db.create_nip46_token("hash-1", SIGNER, ADMIN, "x", &["sign_event:1".to_string()], now, now + Duration::hours(1)).unwrap();
    assert!(db.revoke_nip46_token_as(&token.id, Some(ADMIN), now).unwrap());
    let revoked = last(&db);
    assert_eq!(revoked.action, action::TOKEN_REVOKED);
    assert_eq!((revoked.actor_pubkey.as_deref(), revoked.member_pubkey.as_deref()), (Some(ADMIN), Some(SIGNER)));
    assert_eq!(revoked.token_id.as_deref(), Some(token.id.as_str()));
}

#[test]
fn logout_and_a_replaced_connection_are_recorded_with_their_reason() {
    let db = db();
    let first = connect(&db, "hash-1");
    let second = connect(&db, "hash-2");
    let replaced = db.audit_events().unwrap().into_iter().find(|e| e.action == action::CONNECTION_REVOKED).unwrap();
    assert_eq!(replaced.connection_id.as_deref(), Some(first.as_str()));
    assert_eq!(replaced.detail.as_deref(), Some("reason=replaced"));

    assert!(db.revoke_nip46_connection(&second, "logout", Utc::now()).unwrap());
    let logout = last(&db);
    assert_eq!((logout.connection_id.as_deref(), logout.detail.as_deref()), (Some(second.as_str()), Some("reason=logout")));
    assert_eq!(logout.actor_pubkey, None);
}

#[test]
fn removing_a_member_records_the_removal_and_each_revocation_and_keeps_their_name() {
    let db = db();
    connect(&db, "hash-1");
    let now = Utc::now();
    db.create_nip46_token("hash-2", SIGNER, ADMIN, "y", &["sign_event:1".to_string()], now, now + Duration::hours(1)).unwrap();
    let id = db.find_member_by_pubkey(SIGNER).unwrap().unwrap().id;

    assert_eq!(db.remove_team_member_as(id, Some(ADMIN)).unwrap(), RemoveOutcome::Removed);

    let events = db.audit_events().unwrap();
    let removed = events.iter().find(|e| e.action == action::MEMBER_REMOVED).unwrap();
    assert_eq!(removed.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(removed.member_name.as_deref(), Some("Sam"), "named as they were when removed");
    assert_eq!(removed.detail.as_deref(), Some("role=signer"));
    let revocations: Vec<_> = events.iter().filter(|e| e.detail.as_deref() == Some("reason=member_removed")).map(|e| e.action.as_str()).collect();
    assert_eq!(revocations, [action::CONNECTION_REVOKED, action::TOKEN_REVOKED]);
}

#[test]
fn refusing_to_remove_the_last_administrator_records_nothing() {
    let db = db();
    let before = db.audit_event_count().unwrap();
    let id = db.find_member_by_pubkey(ADMIN).unwrap().unwrap().id;
    assert_eq!(db.remove_team_member_as(id, Some(ADMIN)).unwrap(), RemoveOutcome::LastAdministrator);
    assert_eq!(db.audit_event_count().unwrap(), before);
}

#[test]
fn a_role_change_is_recorded_from_and_to_and_revokes_a_new_viewers_connections() {
    let db = db();
    connect(&db, "hash-1");
    db.change_member_role_as(SIGNER, Role::Viewer, Some(ADMIN)).unwrap();
    let events = db.audit_events().unwrap();
    let changed = events.iter().find(|e| e.action == action::MEMBER_ROLE_CHANGED).unwrap();
    assert_eq!(changed.detail.as_deref(), Some("from=signer;to=viewer"));
    assert_eq!(changed.actor_pubkey.as_deref(), Some(ADMIN));
    assert!(events.iter().any(|e| e.action == action::CONNECTION_REVOKED && e.detail.as_deref() == Some("reason=role_changed")));
}

#[test]
fn setting_the_role_a_member_already_has_records_nothing() {
    let db = db();
    let before = db.audit_event_count().unwrap();
    db.change_member_role_as(SIGNER, Role::Signer, Some(ADMIN)).unwrap();
    assert_eq!(db.audit_event_count().unwrap(), before);
}

#[test]
fn relays_and_a_restore_are_recorded() {
    let db = db();
    db.replace_bunker_relays(&["wss://a.example".to_string(), "wss://b.example".to_string()], ADMIN, Utc::now()).unwrap();
    let relays = last(&db);
    assert_eq!((relays.action.as_str(), relays.actor_pubkey.as_deref()), (action::RELAYS_CHANGED, Some(ADMIN)));
    assert_eq!(relays.detail.as_deref(), Some("relays=wss://a.example wss://b.example"));

    connect(&db, "hash-1");
    let now = Utc::now();
    db.create_nip46_token("hash-2", SIGNER, ADMIN, "y", &["sign_event:1".to_string()], now, now + Duration::hours(1)).unwrap();
    assert_eq!(db.revoke_all_for_restore(now).unwrap(), (1, 1));
    let events = db.audit_events().unwrap();
    let restored = events.last().unwrap();
    assert_eq!((restored.action.as_str(), restored.detail.as_deref()), (action::RESTORED, Some("connections=1;tokens=1")));
    assert_eq!(events.iter().filter(|e| e.detail.as_deref() == Some("reason=restored")).count(), 2);
}

#[test]
fn paging_walks_the_whole_log_once_even_within_one_timestamp() {
    let db = db();
    let same = at(0);
    for i in 0..5 {
        db.log_signing_event(&format!("same-{}", i), CLIENT, Some(SIGNER), None, 1, same).unwrap();
    }
    for i in 0..4 {
        db.log_signing_event(&format!("later-{}", i), CLIENT, Some(SIGNER), None, 1, at(i + 1)).unwrap();
    }

    let mut seen = Vec::new();
    let mut before: Option<Cursor> = None;
    loop {
        let page = db.logs_page(before.as_ref(), 2).unwrap();
        if page.is_empty() {
            break;
        }
        before = Some(Cursor::of(page.last().unwrap()));
        seen.extend(page.into_iter().map(|l| (l.timestamp, l.event_id)));
    }
    assert_eq!(seen.len(), 9);
    let mut sorted = seen.clone();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));
    assert_eq!(seen.iter().map(|s| s.0).collect::<Vec<_>>(), sorted.iter().map(|s| s.0).collect::<Vec<_>>(), "newest first");
    let mut ids: Vec<_> = seen.iter().map(|s| s.1.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 9, "no row twice");
}

#[test]
fn a_cursor_round_trips_and_refuses_anything_else() {
    let cursor = Cursor { timestamp: db_time(at(0)), id: "6f1c2c1e-6a0f-4c9e-9a3e-5b8d1f2a7c10".to_string() };
    assert_eq!(Cursor::decode(&cursor.encode()), Some(cursor.clone()));
    // The console builds it from a log entry's RFC 3339 timestamp, in any offset.
    assert_eq!(Cursor::decode(&format!("2026-06-01T13:00:00+01:00|{}", cursor.id)), Some(cursor.clone()));
    for bad in ["", "no-bar", "2026-06-01|not-a-uuid", "yesterday|6f1c2c1e-6a0f-4c9e-9a3e-5b8d1f2a7c10", "' OR 1=1 --|x"] {
        assert_eq!(Cursor::decode(bad), None, "{bad}");
    }
}

#[test]
fn an_export_batch_holds_signatures_and_audit_events_in_the_range_in_time_order() {
    let db = Database::new(":memory:").unwrap();
    let day = |d| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();
    db.log_signing_event("before", CLIENT, None, None, 1, Utc.with_ymd_and_hms(2026, 5, 31, 23, 59, 59).unwrap()).unwrap();
    db.log_signing_event("first", CLIENT, Some(SIGNER), Some("conn"), 7, Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()).unwrap();
    db.log_signing_event("last", CLIENT, None, None, 1, Utc.with_ymd_and_hms(2026, 6, 2, 23, 59, 59).unwrap()).unwrap();
    db.log_signing_event("after", CLIENT, None, None, 1, Utc.with_ymd_and_hms(2026, 6, 3, 0, 0, 0).unwrap()).unwrap();
    db.record_export(Some(ADMIN), "csv", day(1), day(2), Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()).unwrap();

    let (start, end) = date_bounds(day(1), day(2)).unwrap();
    let rows = db.export_batch(&start, &end, None, 100).unwrap();
    let names: Vec<_> = rows.iter().map(|r| r.event_id.clone().unwrap_or_else(|| r.action.clone())).collect();
    assert_eq!(names, ["first", action::LOGS_EXPORTED, "last"]);
    assert_eq!((rows[0].event_kind, rows[0].action.as_str(), rows[0].source), (Some(7), "signed", Source::Signature));
    assert_eq!(rows[0].member_pubkey.as_deref(), Some(SIGNER));
    assert_eq!(rows[0].client_pubkey.as_deref(), Some(CLIENT));
    assert_eq!(rows[1].detail.as_deref(), Some("format=csv;from=2026-06-01;to=2026-06-02"));

    // In batches of one, continuing from each row's cursor, the same rows come back.
    let mut batched = Vec::new();
    let mut after = None;
    loop {
        let batch = db.export_batch(&start, &end, after.as_ref(), 1).unwrap();
        let Some(row) = batch.into_iter().next() else { break };
        after = Some(row.cursor());
        batched.push(row);
    }
    assert_eq!(batched, rows);
    assert!(date_bounds(day(2), day(1)).is_none(), "a reversed range");
}

#[test]
fn csv_quotes_where_needed_and_defuses_formulas() {
    assert_eq!(csv_field("plain"), "plain");
    assert_eq!(csv_field("a,b"), "\"a,b\"");
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    for formula in ["=HYPERLINK(\"x\")", "+1", "-1+2", "@SUM(A1)", "\tx", "\rx"] {
        assert!(csv_field(formula).trim_start_matches('"').starts_with('\''), "{formula:?}");
    }
    assert_eq!(csv_field("=1,2"), "\"'=1,2\"");

    let db = Database::new(":memory:").unwrap();
    db.add_team_member_as("=cmd|' /C calc'!A0", SIGNER, Role::Signer, None).unwrap();
    let row = db.audit_events().unwrap().pop().unwrap();
    let line = csv_line(&row);
    assert!(line.ends_with("\r\n"));
    assert!(line.contains(",'=cmd|' /C calc'!A0,"), "{line}");
}

#[test]
fn retention_refuses_values_under_the_floors() {
    assert_eq!(Retention::default().validate(), Ok(()));
    assert_eq!(Retention { max_age_days: Some(30), max_rows: Some(10_000) }.validate(), Ok(()));
    assert_eq!(Retention { max_age_days: Some(29), max_rows: None }.validate(), Err(RetentionRefusal::AgeOutOfRange));
    assert_eq!(Retention { max_age_days: Some(0), max_rows: None }.validate(), Err(RetentionRefusal::AgeOutOfRange));
    assert_eq!(Retention { max_age_days: None, max_rows: Some(9_999) }.validate(), Err(RetentionRefusal::RowsOutOfRange));
    assert_eq!(Retention { max_age_days: Some(36_501), max_rows: None }.validate(), Err(RetentionRefusal::AgeOutOfRange));
}

#[test]
fn without_a_policy_nothing_is_pruned() {
    let db = db();
    db.log_signing_event("ancient", CLIENT, None, None, 1, Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()).unwrap();
    let before = db.audit_event_count().unwrap();
    assert_eq!(db.prune_logs(Utc::now()).unwrap(), audit::Pruned::default());
    assert_eq!(db.signature_count().unwrap(), 1);
    assert_eq!(db.audit_event_count().unwrap(), before, "and no logs_pruned");
    assert_eq!(db.log_retention().unwrap(), Retention::default());
}

#[test]
fn pruning_by_age_removes_older_rows_from_both_tables_and_is_recorded() {
    let db = db();
    let now = Utc::now();
    db.log_signing_event("old", CLIENT, None, None, 1, now - Duration::days(31)).unwrap();
    db.log_signing_event("new", CLIENT, None, None, 1, now - Duration::days(29)).unwrap();
    let audit_before = db.audit_event_count().unwrap();
    db.set_log_retention(Retention { max_age_days: Some(30), max_rows: None }, Some(ADMIN), now - Duration::days(40)).unwrap();

    let pruned = db.prune_logs(now).unwrap();

    assert_eq!(pruned.signatures, 1);
    assert_eq!(pruned.audit_events, 1, "the retention change, recorded 40 days ago");
    assert_eq!(db.get_recent_logs(10).unwrap().iter().map(|l| l.event_id.as_str()).collect::<Vec<_>>(), ["new"]);
    let recorded = last(&db);
    assert_eq!(recorded.action, action::LOGS_PRUNED);
    assert_eq!(recorded.detail.as_deref(), Some("signatures=1;audit_events=1"));
    // The retention change was added and pruned; logs_pruned was added.
    assert_eq!(db.audit_event_count().unwrap(), audit_before + 1);
}

#[test]
fn pruning_by_count_keeps_the_newest_rows_in_each_table() {
    let db = Database::new(":memory:").unwrap();
    let now = Utc::now();
    for i in 0..10_050 {
        db.log_signing_event(&format!("e{:05}", i), CLIENT, None, None, 1, now - Duration::seconds(20_000 - i)).unwrap();
    }
    db.set_log_retention(Retention { max_age_days: None, max_rows: Some(10_000) }, None, now).unwrap();

    let pruned = db.prune_logs(now).unwrap();

    assert_eq!(pruned.signatures, 50);
    assert_eq!(db.signature_count().unwrap(), 10_000);
    assert_eq!(db.get_recent_logs(1).unwrap()[0].event_id, "e10049", "the newest stay");
    let (start, end) = date_bounds(NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2100, 1, 1).unwrap()).unwrap();
    assert_eq!(db.export_batch(&start, &end, None, 1).unwrap()[0].event_id.as_deref(), Some("e00050"), "the oldest went");
    assert_eq!(db.prune_logs(now).unwrap(), audit::Pruned::default(), "nothing more to do");
}

#[test]
fn a_retention_change_is_recorded_and_unset_clears_it() {
    let db = db();
    let policy = Retention { max_age_days: Some(90), max_rows: Some(50_000) };
    db.set_log_retention(policy, Some(ADMIN), Utc::now()).unwrap();
    assert_eq!(db.log_retention().unwrap(), policy);
    let changed = last(&db);
    assert_eq!((changed.action.as_str(), changed.actor_pubkey.as_deref()), (action::RETENTION_CHANGED, Some(ADMIN)));
    assert_eq!(changed.detail.as_deref(), Some("max_age_days=90;max_rows=50000"));

    db.set_log_retention(Retention::default(), Some(ADMIN), Utc::now()).unwrap();
    assert_eq!(db.log_retention().unwrap(), Retention::default());
    assert_eq!(last(&db).detail.as_deref(), Some("max_age_days=none;max_rows=none"));
    assert!(db.set_log_retention(Retention { max_age_days: Some(1), max_rows: None }, None, Utc::now()).is_err());
}

#[test]
fn the_trail_lists_events_in_the_order_they_happened() {
    let db = db();
    connect(&db, "hash-1");
    assert_eq!(actions(&db), [action::MEMBER_ADDED, action::MEMBER_ADDED, action::TOKEN_ISSUED, action::CONNECTION_CREATED]);
}
