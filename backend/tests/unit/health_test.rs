// The bunker's health model (#27). Failures are induced, not asserted from a literal: a real
// relay that is up, a port nothing listens on, a database whose table is gone.
use bunker::config::Config;
use bunker::db::Database;
use bunker::registry::Role;
use bunker::health::{self, assess, relay_check, Check, CheckStatus, Overall};
use bunker::relay::RelayClient;
use bunker::signer::Signer;
use bunker::state::AppState;
use nostr::prelude::*;
use nostr_relay_builder::MockRelay;
use std::collections::HashSet;
use std::time::Duration;

fn check(status: CheckStatus) -> Check {
    Check { name: "x".to_string(), status, detail: String::new(), relays: None }
}

/// A bunker with an administrator registered, as a configured one has (#74).
fn state(db: Database, nip46_enabled: bool, relay_urls: Vec<String>) -> AppState {
    db.add_team_member("Admin", &Keys::generate().public_key().to_hex(), Role::Administrator).unwrap();
    bare_state(db, nip46_enabled, relay_urls)
}

/// A bunker exactly as given: no administrator unless the database has one.
fn bare_state(db: Database, nip46_enabled: bool, relay_urls: Vec<String>) -> AppState {
    let keys = Keys::generate();
    let config = Config {
        secret_key: keys.secret_key().clone(),
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls,
        nip46_enabled,
        nsec_file: None,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: None,
    };
    AppState::new(Signer::new(keys.secret_key().clone()), db, config)
}

/// A ws:// URL on a port nothing listens on.
fn closed_relay() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("ws://127.0.0.1:{}", port)
}

/// Starts the NIP-46 relay client as main.rs does, so the pool is the real one.
async fn start_relays(state: &AppState, urls: Vec<String>) {
    let client = RelayClient::new(urls, state.clone()).await.expect("relay client");
    tokio::spawn(async move {
        let _ = client.run().await;
    });
}

/// Polls the report until `done`, as the relays connect in the background.
async fn report_until(state: &AppState, done: impl Fn(&(Overall, Vec<Check>)) -> bool) -> (Overall, Vec<Check>) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let report = health::report(state).await;
        if done(&report) || tokio::time::Instant::now() > deadline {
            return report;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn named<'a>(checks: &'a [Check], name: &str) -> &'a Check {
    checks.iter().find(|check| check.name == name).expect(name)
}

#[test]
fn assess_is_red_on_any_failure_then_yellow_on_any_warning() {
    use CheckStatus::*;
    assert_eq!(assess(&[check(Pass), check(Pass), check(Pass)]), Overall::Healthy);
    assert_eq!(assess(&[check(Pass), check(Disabled)]), Overall::Healthy);
    assert_eq!(assess(&[check(Pass), check(Warn)]), Overall::Degraded);
    assert_eq!(assess(&[check(Warn), check(Fail)]), Overall::Unhealthy);
    assert_eq!(assess(&[check(Pass), check(Fail), check(Disabled)]), Overall::Unhealthy);
}

#[test]
fn relays_are_disabled_and_not_counted_when_nip46_is_off() {
    let relays = relay_check(false, &["wss://relay.example".to_string()], &HashSet::new());
    assert_eq!(relays.status, CheckStatus::Disabled);
    assert_eq!(relays.relays, None);
    assert_eq!(assess(&[relays]), Overall::Healthy);
}

#[test]
fn nip46_on_with_no_relays_configured_fails() {
    assert_eq!(relay_check(true, &[], &HashSet::new()).status, CheckStatus::Fail);
}

#[test]
fn relays_pass_when_all_connected_warn_when_some_and_fail_when_none() {
    let configured = vec!["wss://a.example".to_string(), "wss://b.example".to_string()];
    let url = |s: &str| RelayUrl::parse(s).unwrap();

    let all: HashSet<_> = [url("wss://a.example"), url("wss://b.example")].into();
    assert_eq!(relay_check(true, &configured, &all).status, CheckStatus::Pass);

    let some = relay_check(true, &configured, &[url("wss://a.example")].into());
    assert_eq!(some.status, CheckStatus::Warn);
    assert_eq!(some.detail, "1 of 2 relays connected. Not connected: wss://b.example");

    let none = relay_check(true, &configured, &HashSet::new());
    assert_eq!(none.status, CheckStatus::Fail);
    assert!(none.detail.contains("wss://a.example, wss://b.example"), "{}", none.detail);
}

#[tokio::test]
async fn the_signer_signs_and_verifies() {
    let keys = Keys::generate();
    assert_eq!(health::check_signer(&Signer::new(keys.secret_key().clone())).await.status, CheckStatus::Pass);
}

#[tokio::test]
async fn a_database_that_lost_its_audit_log_fails_and_names_no_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bancwr.db");
    let path = path.to_str().unwrap();
    let state = state(Database::new(path).unwrap(), false, vec![]);
    assert_eq!(health::report(&state).await.0, Overall::Healthy);

    // Break it from outside, through a second connection.
    rusqlite::Connection::open(path).unwrap().execute_batch("DROP TABLE signing_logs;").unwrap();

    let (overall, checks) = health::report(&state).await;
    assert_eq!(overall, Overall::Unhealthy);
    let database = named(&checks, health::DATABASE);
    assert_eq!(database.status, CheckStatus::Fail);
    // Sanitised: every role can read this.
    assert!(!database.detail.contains(path) && !database.detail.contains("signing_logs"), "{}", database.detail);
}

#[tokio::test]
async fn one_relay_up_and_one_down_is_degraded_and_names_the_one_down() {
    let relay = MockRelay::run().await.unwrap();
    let down = closed_relay();
    let urls = vec![relay.url(), down.clone()];
    let state = state(Database::new(":memory:").unwrap(), true, urls.clone());
    start_relays(&state, urls).await;

    let (overall, checks) = report_until(&state, |(overall, _)| *overall == Overall::Degraded).await;

    assert_eq!(overall, Overall::Degraded);
    let relays = named(&checks, health::RELAYS);
    assert_eq!(relays.status, CheckStatus::Warn);
    assert_eq!(relays.detail, format!("1 of 2 relays connected. Not connected: {}", down));
    let listed = relays.relays.as_ref().unwrap();
    assert!(listed.iter().any(|r| r.url == relay.url() && r.connected));
    assert!(listed.iter().any(|r| r.url == down && !r.connected));
}

#[tokio::test]
async fn every_relay_up_is_healthy() {
    let relay = MockRelay::run().await.unwrap();
    let state = state(Database::new(":memory:").unwrap(), true, vec![relay.url()]);
    start_relays(&state, vec![relay.url()]).await;

    let (overall, checks) = report_until(&state, |(overall, _)| *overall == Overall::Healthy).await;

    assert_eq!(overall, Overall::Healthy);
    assert!(checks.iter().all(|check| check.status == CheckStatus::Pass), "{:?}", checks);
}

#[tokio::test]
async fn no_relay_reachable_is_unhealthy() {
    let down = closed_relay();
    let state = state(Database::new(":memory:").unwrap(), true, vec![down.clone()]);
    start_relays(&state, vec![down]).await;
    // Give the pool time to try, so this is a failed connection rather than one not yet made.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (overall, checks) = health::report(&state).await;

    assert_eq!(overall, Overall::Unhealthy);
    assert_eq!(named(&checks, health::RELAYS).status, CheckStatus::Fail);
}

#[tokio::test]
async fn no_administrator_is_degraded_and_says_how_to_set_one() {
    let state = bare_state(Database::new(":memory:").unwrap(), false, vec![]);

    let (overall, checks) = health::report(&state).await;

    assert_eq!(overall, Overall::Degraded);
    let administrator = named(&checks, health::ADMINISTRATOR);
    assert_eq!(administrator.status, CheckStatus::Warn);
    assert!(administrator.detail.contains("BANCWR_ADMIN_PUBKEY"), "{}", administrator.detail);
    assert!(administrator.detail.contains("never the bunker's own key"), "{}", administrator.detail);
}

#[tokio::test]
async fn an_administrator_passes_and_members_of_other_roles_do_not_count() {
    let db = Database::new(":memory:").unwrap();
    db.add_team_member("Viewer", &Keys::generate().public_key().to_hex(), Role::Viewer).unwrap();
    db.add_team_member("Signer", &Keys::generate().public_key().to_hex(), Role::Signer).unwrap();
    assert_eq!(health::check_administrator(&db).status, CheckStatus::Warn);

    db.add_team_member("Admin", &Keys::generate().public_key().to_hex(), Role::Administrator).unwrap();
    assert_eq!(health::check_administrator(&db).status, CheckStatus::Pass);
}

#[tokio::test]
async fn the_report_has_all_four_checks() {
    let state = state(Database::new(":memory:").unwrap(), false, vec![]);
    let (_, checks) = health::report(&state).await;
    let names: Vec<&str> = checks.iter().map(|check| check.name.as_str()).collect();
    assert_eq!(names, vec![health::SIGNER, health::DATABASE, health::RELAYS, health::ADMINISTRATOR]);
}
