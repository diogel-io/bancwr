// The bunker's own relays (#78) through the API: only administrators read and change them; a list
// is validated and saved whole; NIP46_RELAYS, when set, decides and cannot be changed here; and
// the relays in force are the ones token bunker:// strings carry, the health check reports and
// the running relay client uses, without a restart.
#[path = "../common/mod.rs"]
mod common;

use bunker::bunker_relays::{normalise_relay_url, MAX_RELAYS};
use bunker::connections_api::IssueTokenResponse;
use bunker::registry::Role;
use bunker::relay::RelayClient;
use bunker::relays_api::RelaysResponse;
use bunker::bunker_relays::RelaySource;
use nostr_relay_builder::MockRelay;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

const PATH: &str = "/api/bunker/relays";

/// NIP-46 on and NIP46_RELAYS unset: the console's list is in force.
async fn console() -> common::TestApp {
    common::spawn_with(true, Some(vec![])).await
}

async fn put(app: &common::TestApp, relays: Value) -> reqwest::Response {
    app.request(Method::PUT, PATH).json(&json!({ "relays": relays })).send().await.unwrap()
}

async fn get(app: &common::TestApp) -> RelaysResponse {
    let res = app.get(PATH).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

fn urls(response: &RelaysResponse) -> Vec<&str> {
    response.relays.iter().map(|r| r.url.as_str()).collect()
}

async fn issue_token(app: &common::TestApp) -> reqwest::Response {
    app.request(Method::POST, "/api/bunker/connections/tokens").json(&json!({ "label": "x", "kinds": [1] })).send().await.unwrap()
}

/// The relay check from GET /api/bunker/status.
async fn relay_check(app: &common::TestApp) -> Value {
    let status: Value = app.get("/api/bunker/status").send().await.unwrap().json().await.unwrap();
    status["checks"].as_array().unwrap().iter().find(|c| c["name"] == "relays").unwrap().clone()
}

#[tokio::test]
async fn only_administrators_read_and_change_the_bunker_relays() {
    let app = console().await;
    let signer = app.register(Role::Signer).public_key().to_hex();
    let viewer = app.register(Role::Viewer).public_key().to_hex();

    for who in [&signer, &viewer] {
        assert_eq!(app.signed(Method::GET, PATH, who).send().await.unwrap().status(), StatusCode::FORBIDDEN);
        let res = app.signed(Method::PUT, PATH, who).json(&json!({ "relays": ["wss://a.example"] })).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    // Unsigned: the guard refuses before any handler runs.
    assert_eq!(app.unsigned(Method::GET, PATH).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert!(app.db.list_bunker_relays().unwrap().is_empty(), "nothing was saved by a refused caller");

    let listed = get(&app).await;
    assert_eq!(listed.source, RelaySource::Console);
    assert!(listed.nip46_enabled);
    assert!(listed.relays.is_empty());
}

#[tokio::test]
async fn an_administrator_saves_a_list_normalised_in_their_order() {
    let app = console().await;

    let res = put(&app, json!(["wss://B.example/", " wss://a.example ", "ws://localhost:7777", "ws://127.0.0.1:7000/path", "ws://[::1]:7001"])).await;
    assert_eq!(res.status(), StatusCode::OK);
    let saved: RelaysResponse = res.json().await.unwrap();
    let expected = vec!["wss://b.example", "wss://a.example", "ws://localhost:7777", "ws://127.0.0.1:7000/path", "ws://[::1]:7001"];
    assert_eq!(urls(&saved), expected);
    assert_eq!(saved.source, RelaySource::Console);
    assert!(saved.relays.iter().all(|r| !r.connected), "no relay client is running");
    assert_eq!(urls(&get(&app).await), expected);

    // Recorded as the administrator's.
    let stored = app.db.list_bunker_relays().unwrap();
    assert!(stored.iter().all(|r| r.added_by == app.admin.public_key().to_hex()));

    // Replaced whole.
    assert_eq!(put(&app, json!(["wss://c.example"])).await.status(), StatusCode::OK);
    assert_eq!(urls(&get(&app).await), vec!["wss://c.example"]);
}

#[tokio::test]
async fn every_refusal_is_a_400_with_its_own_code_and_saves_nothing() {
    let app = console().await;
    assert_eq!(put(&app, json!(["wss://kept.example"])).await.status(), StatusCode::OK);

    let seven: Vec<String> = (0..=MAX_RELAYS).map(|i| format!("wss://r{}.example", i)).collect();
    for (relays, error) in [
        (json!(["https://relay.example"]), "invalid_relay_url"),
        (json!(["relay.example"]), "invalid_relay_url"),
        (json!([""]), "invalid_relay_url"),
        (json!(["ws://relay.example"]), "invalid_relay_url"),
        (json!(["ws://192.168.1.10:7777"]), "invalid_relay_url"),
        (json!(["wss://relay.example/#x"]), "invalid_relay_url"),
        (json!(["wss://user:pass@relay.example"]), "invalid_relay_url"),
        (json!(["wss://a.example", "wss://A.example/"]), "duplicate_relay"),
        (json!(seven), "too_many_relays"),
        (json!([]), "no_relays"),
    ] {
        let res = put(&app, relays.clone()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{}", relays);
        let body: Value = res.json().await.unwrap();
        assert_eq!(body["error"], error, "{}", relays);
        assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()), "{}", relays);
    }
    assert_eq!(urls(&get(&app).await), vec!["wss://kept.example"], "every refusal left the list as it was");

    // Six is the most, and allowed.
    let six: Vec<String> = (0..MAX_RELAYS).map(|i| format!("wss://r{}.example", i)).collect();
    assert_eq!(put(&app, json!(six)).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn with_nip46_off_an_empty_list_may_be_saved() {
    let app = common::spawn(true).await;
    assert_eq!(put(&app, json!(["wss://a.example"])).await.status(), StatusCode::OK);
    assert_eq!(put(&app, json!([])).await.status(), StatusCode::OK);
    let listed = get(&app).await;
    assert!(!listed.nip46_enabled);
    assert!(listed.relays.is_empty());
}

#[tokio::test]
async fn nip46_relays_decides_and_cannot_be_changed_here() {
    let app = common::spawn_with(true, Some(vec!["wss://env.example".to_string()])).await;
    // A list stored earlier, from the console: ignored while the variable is set.
    app.db.replace_bunker_relays(&["wss://stored.example".to_string()], "someone", chrono::Utc::now()).unwrap();

    let listed = get(&app).await;
    assert_eq!(listed.source, RelaySource::Environment);
    assert_eq!(urls(&listed), vec!["wss://env.example"]);

    let res = put(&app, json!(["wss://new.example"])).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"], "relays_from_environment");
    assert!(body["message"].as_str().unwrap().contains("NIP46_RELAYS"));
    // Refused before validation, too: even a valid list changes nothing.
    assert_eq!(app.db.list_bunker_relays().unwrap()[0].url, "wss://stored.example");

    // Tokens and health use the variable's relays, not the stored ones.
    let token: IssueTokenResponse = issue_token(&app).await.json().await.unwrap();
    assert!(token.uri.contains("relay=wss%3A%2F%2Fenv.example&"), "{}", token.uri);
    assert!(!token.uri.contains("stored.example"), "{}", token.uri);
    let check = relay_check(&app).await;
    assert_eq!(check["relays"][0]["url"], "wss://env.example");
    assert_eq!(check["relays"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn tokens_and_the_health_check_use_the_console_list() {
    let app = console().await;

    // Nothing configured from either source: no token, and the check names both.
    let res = issue_token(&app).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"], "nip46_disabled");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("NIP46_RELAYS") && message.contains("Bunker relays"), "{}", message);
    let check = relay_check(&app).await;
    assert_eq!(check["status"], "fail");
    assert_eq!(check["detail"], bunker::health::NO_RELAYS);

    assert_eq!(put(&app, json!(["wss://b.example", "wss://a.example"])).await.status(), StatusCode::OK);

    // A new token carries the saved relays, in order.
    let token: IssueTokenResponse = issue_token(&app).await.json().await.unwrap();
    assert!(
        token.uri.contains("?relay=wss%3A%2F%2Fb.example&relay=wss%3A%2F%2Fa.example&secret="),
        "{}",
        token.uri
    );
    // And the health check reports them (not connected: no relay client runs here).
    let check = relay_check(&app).await;
    let reported: Vec<&str> = check["relays"].as_array().unwrap().iter().map(|r| r["url"].as_str().unwrap()).collect();
    assert_eq!(reported, vec!["wss://b.example", "wss://a.example"]);
}

/// The relays GET reports connected, once `done` holds (or 10 s).
async fn relays_until(app: &common::TestApp, done: impl Fn(&RelaysResponse) -> bool) -> RelaysResponse {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let listed = get(app).await;
        if done(&listed) || tokio::time::Instant::now() > deadline {
            return listed;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_saved_list_applies_to_the_running_relay_client_without_a_restart() {
    let first = MockRelay::run().await.unwrap();
    let second = MockRelay::run().await.unwrap();
    let app = console().await;
    // As main.rs starts it with NIP-46 on and nothing configured yet.
    let client = RelayClient::new(vec![], app.state.clone()).await.unwrap();
    tokio::spawn(async move {
        let _ = client.run().await;
    });

    assert_eq!(put(&app, json!([first.url()])).await.status(), StatusCode::OK);
    let listed = relays_until(&app, |l| l.relays.iter().all(|r| r.connected)).await;
    assert_eq!(listed.relays.len(), 1);
    assert!(listed.relays[0].connected, "{:?}", listed.relays);
    let check = relay_check(&app).await;
    assert_eq!(check["status"], "pass", "{}", check);

    assert_eq!(put(&app, json!([second.url()])).await.status(), StatusCode::OK);
    let listed = relays_until(&app, |l| l.relays.iter().all(|r| r.connected)).await;
    assert_eq!(urls(&listed), vec![second.url().as_str()]);
    assert!(listed.relays[0].connected);
    // The pool holds only the new relay: the old one was removed, not just left unlisted.
    let pool = app.state.relay_pool().await.unwrap();
    let held: Vec<String> = pool.relays().await.keys().map(|u| u.to_string()).collect();
    assert_eq!(held, vec![second.url()]);
}

#[test]
fn relay_addresses_are_normalised_as_the_console_normalises_them() {
    assert_eq!(normalise_relay_url(" WSS://Relay.Example.com/ "), Ok("wss://relay.example.com".to_string()));
    assert_eq!(normalise_relay_url("wss://relay.example.com/inbox"), Ok("wss://relay.example.com/inbox".to_string()));
    assert_eq!(normalise_relay_url("wss://relay.example.com:443"), Ok("wss://relay.example.com".to_string()));
    assert_eq!(normalise_relay_url("ws://LOCALHOST:7777/"), Ok("ws://localhost:7777".to_string()));
    assert!(normalise_relay_url(&format!("wss://{}.example", "a".repeat(250))).is_err());
}
