// The approval queue (#102): its bounds, ordering, expiry, taking a request once, and the wording
// an app is given. The handler's use of it is in nip46_test.rs.
use bunker::approvals::{
    clean_note, describe_timeout, rejected_message, timed_out_message, ApprovalQueue, Pending, QueueFull, Scheme, MAX_NOTE_CHARS,
    MAX_PENDING_PER_CONNECTION,
};
use bunker::db::Nip46Connection;
use chrono::{Duration, Utc};
use nostr::prelude::*;
use uuid::Uuid;

fn connection(id: &str, member: &str) -> Nip46Connection {
    Nip46Connection {
        id: id.to_string(),
        client_pubkey: Keys::generate().public_key().to_hex(),
        token_id: "t".to_string(),
        for_pubkey: member.to_string(),
        perms: vec!["sign_event:1".to_string()],
        client_name: None,
        client_url: None,
        client_image: None,
        connected_at: Utc::now(),
        last_used_at: None,
        revoked_at: None,
        revoked_reason: None,
        revoked_by: None,
        expires_at: None,
        rate_limit: None,
        approval_perms: vec!["sign_event:1".to_string()],
        identity_pubkey: None,
    }
}

fn pending(connection: &Nip46Connection, requested_ago: Duration, waits: Duration) -> Pending {
    let keys = Keys::generate();
    let mut event = EventBuilder::text_note("hello").build(keys.public_key());
    event.ensure_id();
    let requested_at = Utc::now() - requested_ago;
    Pending {
        id: Uuid::new_v4(),
        request_id: "r".to_string(),
        client_pubkey: keys.public_key(),
        scheme: Scheme::Nip44,
        connection: connection.clone(),
        event_id: event.id.unwrap(),
        event,
        requested_at,
        expires_at: requested_at + waits,
    }
}

#[test]
fn holds_a_request_and_gives_it_to_whoever_takes_it_once() {
    let queue = ApprovalQueue::new();
    let p = pending(&connection("c1", "m1"), Duration::zero(), Duration::minutes(30));
    let id = p.id;
    queue.hold(p).unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.get(id).map(|p| p.id), Some(id));
    assert!(queue.take(id).is_some());
    assert!(queue.take(id).is_none(), "decided once");
    assert!(queue.is_empty());
}

#[test]
fn refuses_more_than_the_limit_for_one_connection_but_not_for_another() {
    let queue = ApprovalQueue::new();
    let busy = connection("c1", "m1");
    for _ in 0..MAX_PENDING_PER_CONNECTION {
        queue.hold(pending(&busy, Duration::zero(), Duration::minutes(30))).unwrap();
    }
    assert_eq!(queue.hold(pending(&busy, Duration::zero(), Duration::minutes(30))), Err(QueueFull::Connection));
    assert!(queue.hold(pending(&connection("c2", "m1"), Duration::zero(), Duration::minutes(30))).is_ok());
}

#[test]
fn refuses_more_than_the_limit_for_the_bunker() {
    let queue = ApprovalQueue::new();
    for n in 0..bunker::approvals::MAX_PENDING {
        queue.hold(pending(&connection(&format!("c{}", n), "m1"), Duration::zero(), Duration::minutes(30))).unwrap();
    }
    assert_eq!(queue.hold(pending(&connection("one-more", "m1"), Duration::zero(), Duration::minutes(30))), Err(QueueFull::Bunker));
}

#[test]
fn lists_oldest_first_and_only_a_members_own_when_asked() {
    let queue = ApprovalQueue::new();
    let newer = pending(&connection("c1", "m1"), Duration::minutes(1), Duration::minutes(30));
    let older = pending(&connection("c2", "m2"), Duration::minutes(5), Duration::minutes(30));
    let (newer_id, older_id) = (newer.id, older.id);
    queue.hold(newer).unwrap();
    queue.hold(older).unwrap();
    assert_eq!(queue.list(None).iter().map(|p| p.id).collect::<Vec<_>>(), vec![older_id, newer_id]);
    assert_eq!(queue.list(Some("m1")).iter().map(|p| p.id).collect::<Vec<_>>(), vec![newer_id]);
    assert!(queue.list(Some("nobody")).is_empty());
}

#[test]
fn takes_only_the_requests_that_have_expired() {
    let queue = ApprovalQueue::new();
    let due = pending(&connection("c1", "m1"), Duration::minutes(31), Duration::minutes(30));
    let waiting = pending(&connection("c2", "m1"), Duration::minutes(1), Duration::minutes(30));
    let (due_id, waiting_id) = (due.id, waiting.id);
    queue.hold(due).unwrap();
    queue.hold(waiting).unwrap();
    assert_eq!(queue.take_expired(Utc::now()).iter().map(|p| p.id).collect::<Vec<_>>(), vec![due_id]);
    assert_eq!(queue.list(None).iter().map(|p| p.id).collect::<Vec<_>>(), vec![waiting_id]);
}

#[test]
fn the_audit_detail_names_the_request_kind_and_event_and_never_the_content() {
    let p = pending(&connection("c1", "m1"), Duration::zero(), Duration::minutes(30));
    let detail = p.audit_detail();
    assert_eq!(detail, format!("request={};kind=1;event_id={}", p.id, p.event_id.to_hex()));
    assert!(!detail.contains("hello"));
}

#[test]
fn notes_are_one_trimmed_line_of_at_most_280_characters() {
    assert_eq!(clean_note(None), None);
    assert_eq!(clean_note(Some("   ")), None);
    assert_eq!(clean_note(Some("  not\nthis\tone  ")), Some("not this one".to_string()));
    assert_eq!(clean_note(Some(&"x".repeat(500))).unwrap().chars().count(), MAX_NOTE_CHARS);
}

#[test]
fn the_app_is_told_why_nothing_was_signed() {
    assert_eq!(rejected_message(None), "Rejected by an administrator");
    assert_eq!(rejected_message(Some("Wrong price")), "Rejected by an administrator: Wrong price");
    assert_eq!(timed_out_message(Duration::minutes(30)), "Approval timed out after 30 minutes: nothing was signed");
    assert_eq!(describe_timeout(Duration::minutes(1)), "1 minute");
    assert_eq!(describe_timeout(Duration::seconds(45)), "45 seconds");
}
