// Connecting an app from its own nostrconnect:// code (#148), end to end over real relays: a
// client that behaves as nostr-tools' BunkerSigner.fromURI does (2.25: NIP-44, the secret as the
// connect result, then switch_relays at once) shows a code on a relay the bunker does not use; a
// member connects it from the console; the app receives the connect response from the identity
// chosen, signs only what was confirmed, moves to the bunker's relays, and is refused once revoked
// or logged out. Plus who may connect whom, the checks on the code, and rust-nostr 0.39's older
// form being refused.
#[path = "../common/mod.rs"]
mod common;
#[path = "../support/ports.rs"]
mod ports;

use bunker::connections_api::{ConnectionResponse, NostrConnectPreview, TokenResponse};
use bunker::registry::Role;
use bunker::relay::RelayClient;
use nostr_connect::prelude::*;
use nostr_sdk::{Client, RelayPoolNotification};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

/// The bunker's NIP-46 relay client on `relay`, started on the app's state as main.rs starts it.
async fn start_bunker(app: &common::TestApp, relay: &str) {
    let client = RelayClient::new(vec![relay.to_string()], app.state.clone()).await.unwrap();
    tokio::spawn(async move {
        let _ = client.run().await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// An app showing a nostrconnect:// code, as nostr-tools' BunkerSigner.fromURI waits for an answer:
/// subscribed on its relays for kind 24133 addressed to its key.
struct App {
    keys: Keys,
    client: Client,
    secret: String,
}

impl App {
    async fn on(relays: &[&str]) -> Self {
        let keys = Keys::generate();
        let client = Client::builder().signer(keys.clone()).build();
        for relay in relays {
            client.add_relay(*relay).await.unwrap();
        }
        client.connect().await;
        client.subscribe(Filter::new().kind(Kind::from(24133)).pubkey(keys.public_key()), None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        Self { keys, client, secret: uuid::Uuid::new_v4().simple().to_string()[..12].to_string() }
    }

    /// Its code, naming `relays`, asking for `perms`.
    fn code(&self, relays: &[&str], perms: &str) -> String {
        let relays: String = relays.iter().map(|r| format!("relay={}&", urlencode(r))).collect();
        format!("nostrconnect://{}?{}secret={}&perms={}&name=Test%20App&url=https://app.example&image=https://tracker.example/logo.png", self.keys.public_key().to_hex(), relays, self.secret, perms)
    }

    /// The next kind 24133 event to it, decrypted with NIP-44, with its author.
    async fn next(&self, notifications: &mut tokio::sync::broadcast::Receiver<RelayPoolNotification>, wait: Duration) -> Option<(PublicKey, Value)> {
        tokio::time::timeout(wait, async {
            while let Ok(notification) = notifications.recv().await {
                let RelayPoolNotification::Event { event, .. } = notification else { continue };
                if event.kind != Kind::from(24133) {
                    continue;
                }
                let Ok(text) = nip44::decrypt(self.keys.secret_key(), &event.pubkey, &event.content) else { continue };
                return Some((event.pubkey, serde_json::from_str(&text).unwrap()));
            }
            None
        })
        .await
        .ok()
        .flatten()
    }

    /// Sends a NIP-44 request to `signer` on `relays`, and returns its answer, or None within `wait`.
    async fn ask_on(&self, relays: &[&str], signer: PublicKey, method: &str, params: Vec<String>, wait: Duration) -> Option<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let body = json!({ "id": id, "method": method, "params": params }).to_string();
        let content = nip44::encrypt(self.keys.secret_key(), &signer, body, nip44::Version::V2).unwrap();
        let mut notifications = self.client.notifications();
        let request = EventBuilder::new(Kind::from(24133), content).tag(Tag::public_key(signer)).sign_with_keys(&self.keys).unwrap();
        self.client.send_event_to(relays.iter().copied(), request).await.unwrap();
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let (author, response) = self.next(&mut notifications, left).await?;
            if author == signer && response["id"] == id {
                return Some(response);
            }
        }
    }

    async fn ask(&self, relays: &[&str], signer: PublicKey, method: &str, params: Vec<String>) -> Value {
        self.ask_on(relays, signer, method, params, Duration::from_secs(10)).await.unwrap_or_else(|| panic!("an answer to {}", method))
    }
}

fn urlencode(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

async fn connect(app: &common::TestApp, as_identity: &str, body: Value) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/nostrconnect", as_identity).json(&body).send().await.unwrap()
}

async fn preview(app: &common::TestApp, as_identity: &str, uri: &str) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/nostrconnect/preview", as_identity).json(&json!({ "uri": uri })).send().await.unwrap()
}

async fn error_of(res: reqwest::Response) -> (StatusCode, String, String) {
    let status = res.status();
    let body: Value = res.json().await.unwrap();
    (status, body["error"].as_str().unwrap_or("").to_string(), body["message"].as_str().unwrap_or("").to_string())
}

fn sign_event_param(signer: PublicKey, kind: u16) -> Vec<String> {
    let unsigned = EventBuilder::new(Kind::from(kind), "from a nostrconnect:// app").build(signer);
    vec![serde_json::to_string(&unsigned).unwrap()]
}

#[tokio::test]
async fn an_app_on_its_own_relay_connects_from_its_code_signs_what_was_confirmed_and_moves_to_the_bunkers_relays() {
    let bunker_relay = ports::run_mock_relay().await;
    let app_relay = ports::run_mock_relay().await;
    let sales = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec![bunker_relay.url()]), &[("Sales", sales.clone())]).await;
    let member = app.register(Role::Signer);
    app.db.grant_identity(Some(&sales.public_key().to_hex()), &member.public_key().to_hex(), None, chrono::Utc::now()).unwrap();
    start_bunker(&app, &bunker_relay.url()).await;

    let (b, a) = (bunker_relay.url(), app_relay.url());
    let client = App::on(&[&a, &b]).await;
    let code = client.code(&[&a], "sign_event:1,sign_event:7,nip44_encrypt");
    let member_hex = member.public_key().to_hex();

    // The preview shows what the app says and asks for, marks the relay as not the bunker's, and
    // carries neither the secret nor the image.
    let res = preview(&app, &member_hex, &code).await;
    assert_eq!(res.status(), StatusCode::OK);
    let text = res.text().await.unwrap();
    assert!(!text.contains(&client.secret) && !text.contains("tracker.example"), "{}", text);
    let shown: NostrConnectPreview = serde_json::from_str(&text).unwrap();
    assert_eq!(shown.client_pubkey, client.keys.public_key().to_hex());
    assert_eq!(shown.name.as_deref(), Some("Test App"));
    assert!(!shown.metadata_verified);
    assert_eq!(shown.requested_kinds, vec![1, 7]);
    assert_eq!(shown.requested_methods, vec!["nip44_encrypt".to_string()]);
    assert_eq!(shown.relays.len(), 1);
    assert!(!shown.relays[0].bunker);
    assert!(!shown.already_connected);

    // The member confirms kind 1 only (and kind 30023, which the app did not ask for) as Sales.
    let mut notifications = client.client.notifications();
    let res = connect(&app, &member_hex, json!({ "uri": code, "identity": sales.public_key().to_bech32().unwrap(), "kinds": [1, 30023] })).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let text = res.text().await.unwrap();
    assert!(!text.contains(&client.secret), "the secret is never returned: {}", text);
    let made: ConnectionResponse = serde_json::from_str(&text).unwrap();
    assert_eq!(made.kinds, vec![1], "only what the member confirmed and the app asked for");
    assert_eq!(made.identity_pubkey.as_deref(), Some(sales.public_key().to_hex().as_str()));
    assert_eq!(made.for_pubkey, member_hex);
    assert_eq!(made.app_relays, vec![a.clone()]);
    assert_eq!(made.client_image, None);

    // The connect response: from Sales, with the app's secret, on the app's relay.
    let (author, response) = client.next(&mut notifications, Duration::from_secs(10)).await.expect("a connect response");
    assert_eq!(author, sales.public_key());
    assert_eq!(response["result"], client.secret.as_str(), "{}", response);

    // On its own relay the app is answered: get_public_key is the identity chosen, kind 1 signs as
    // it, kind 7 and nip44_encrypt were not confirmed.
    let key = client.ask(&[&a], sales.public_key(), "get_public_key", vec![]).await;
    assert_eq!(key["result"], sales.public_key().to_hex());
    let signed = client.ask(&[&a], sales.public_key(), "sign_event", sign_event_param(sales.public_key(), 1)).await;
    let event = Event::from_json(signed["result"].as_str().expect("a signed event")).unwrap();
    assert_eq!(event.pubkey, sales.public_key());
    assert!(event.verify().is_ok());
    let refused = client.ask(&[&a], sales.public_key(), "sign_event", sign_event_param(sales.public_key(), 7)).await;
    assert!(refused["error"].as_str().unwrap().contains("may not sign kind 7"), "{}", refused);
    let encrypt = client.ask(&[&a], sales.public_key(), "nip44_encrypt", vec![Keys::generate().public_key().to_hex(), "hi".into()]).await;
    assert!(encrypt["error"].as_str().unwrap().contains("Forbidden"), "{}", encrypt);

    // switch_relays names the bunker's relays, where the app is answered too.
    let switched = client.ask(&[&a], sales.public_key(), "switch_relays", vec![]).await;
    let relays: Vec<String> = serde_json::from_str(switched["result"].as_str().unwrap()).unwrap();
    assert_eq!(relays, vec![b.clone()]);
    let pong = client.ask(&[&b], sales.public_key(), "ping", vec![]).await;
    assert_eq!(pong["result"], "pong");

    // Listed for the member, signed for them, and the backing token is not listed as a token.
    let listed: Vec<ConnectionResponse> = app.signed(Method::GET, "/api/bunker/connections", &member_hex).send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].last_used_at.is_some());
    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &member_hex).send().await.unwrap().json().await.unwrap();
    assert!(tokens.is_empty(), "{:?}", tokens);
    let logs = app.db.get_recent_logs_for_member(&member_hex, 10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].event_id, event.id.to_hex());

    // Recorded with who made it and how.
    let events = app.db.audit_events().unwrap();
    let created = events.iter().find(|e| e.action == "connection_created").expect("connection_created");
    let detail = created.detail.clone().unwrap_or_default();
    assert_eq!(created.actor_pubkey.as_deref(), Some(member_hex.as_str()));
    assert!(detail.contains("via=nostrconnect") && detail.contains("perms=sign_event:1"), "{}", detail);
    assert!(!detail.contains(&client.secret));

    // Revoked in the console: refused on the bunker's relay, and no longer heard on the app's.
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", made.id), &member_hex).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let after = client.ask(&[&b], sales.public_key(), "sign_event", sign_event_param(sales.public_key(), 1)).await;
    assert!(after["error"].as_str().unwrap().contains("Not connected"), "{}", after);
    let unheard = client.ask_on(&[&a], sales.public_key(), "ping", vec![], Duration::from_secs(2)).await;
    assert!(unheard.is_none(), "the bunker left the app's relay: {:?}", unheard);
}

#[tokio::test]
async fn an_app_that_logs_out_is_refused_afterwards() {
    let relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker(&app, &relay.url()).await;
    let r = relay.url();
    let client = App::on(&[&r]).await;
    let mut notifications = client.client.notifications();
    let admin = app.admin.public_key().to_hex();
    // No perms in the code: the app asked for nothing, so it gets what was confirmed.
    let code = format!("nostrconnect://{}?relay={}&secret={}", client.keys.public_key().to_hex(), urlencode(&r), client.secret);
    let res = connect(&app, &admin, json!({ "uri": code, "kinds": [1, 7] })).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let made: ConnectionResponse = res.json().await.unwrap();
    assert_eq!(made.kinds, vec![1, 7]);
    assert_eq!(made.identity_pubkey.as_deref(), Some(app.bunker.public_key().to_hex().as_str()));
    let (author, response) = client.next(&mut notifications, Duration::from_secs(10)).await.expect("a connect response");
    assert_eq!(author, app.bunker.public_key());
    assert_eq!(response["result"], client.secret.as_str());

    let bunker = app.bunker.public_key();
    assert_eq!(client.ask(&[&r], bunker, "logout", vec![]).await["result"], "ack");
    let after = client.ask(&[&r], bunker, "get_public_key", vec![]).await;
    assert!(after["error"].as_str().unwrap().contains("Not connected"), "{}", after);
    let listed = app.db.list_nip46_connections(None).unwrap();
    assert_eq!(listed[0].revoked_reason.as_deref(), Some("logout"));
}

#[tokio::test]
async fn a_connect_response_no_relay_takes_leaves_no_connection() {
    let relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker(&app, &relay.url()).await;
    let keys = Keys::generate();
    // Nothing listens on this port.
    let dead = format!("ws://127.0.0.1:{}", ports::free_port());
    let code = format!("nostrconnect://{}?relay={}&secret=abc", keys.public_key().to_hex(), urlencode(&dead));
    let (status, error, message) = error_of(connect(&app, &app.admin.public_key().to_hex(), json!({ "uri": code, "kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::BAD_GATEWAY, "connect_undelivered"), "{}", message);
    let listed = app.db.list_nip46_connections(None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].revoked_reason.as_deref(), Some("connect_undelivered"));
    assert!(app.db.active_nip46_connection(&keys.public_key().to_hex()).unwrap().is_none());
}

#[tokio::test]
async fn who_may_connect_whom_follows_the_token_rules() {
    let relay = ports::run_mock_relay().await;
    let sales = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec![relay.url()]), &[("Sales", sales.clone())]).await;
    start_bunker(&app, &relay.url()).await;
    let signer = app.register(Role::Signer).public_key().to_hex();
    let other = app.register(Role::Signer).public_key().to_hex();
    let r = relay.url();
    let code = || format!("nostrconnect://{}?relay={}&secret=s3cret", Keys::generate().public_key().to_hex(), urlencode(&r));

    // A signer only for themselves, only as identities they may use, and without approval kinds.
    let (status, error, _) = error_of(connect(&app, &signer, json!({ "uri": code(), "for_pubkey": other, "kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::FORBIDDEN, "forbidden_member"));
    let (status, error, _) = error_of(connect(&app, &signer, json!({ "uri": code(), "identity": sales.public_key().to_hex(), "kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::BAD_REQUEST, "no_identity_access"));
    let (status, error, _) = error_of(connect(&app, &signer, json!({ "uri": code(), "kinds": [1], "approval_kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::FORBIDDEN, "approval_admin_only"));
    assert!(app.db.list_nip46_connections(None).unwrap().is_empty(), "nothing was made");

    // An administrator may connect for another member, as an identity they may use, with approval
    // kinds. With a second identity held, the primary needs a grant too (#103).
    app.db.grant_identity(None, &signer, None, chrono::Utc::now()).unwrap();
    let res = connect(&app, &app.admin.public_key().to_hex(), json!({ "uri": code(), "for_pubkey": signer, "kinds": [1, 7], "approval_kinds": [7] })).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let made: ConnectionResponse = res.json().await.unwrap();
    assert_eq!((made.for_pubkey.as_str(), made.approval_kinds.clone()), (signer.as_str(), vec![7]));
    // That connection is the signer's to see and revoke.
    let theirs: Vec<ConnectionResponse> = app.signed(Method::GET, "/api/bunker/connections", &signer).send().await.unwrap().json().await.unwrap();
    assert_eq!(theirs.len(), 1);
}

#[tokio::test]
async fn bad_codes_and_repeat_connections_are_refused_with_a_reason() {
    let relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();
    let r = relay.url();
    let key = Keys::generate().public_key().to_hex();

    for (code, expected) in [
        ("not a code".to_string(), "not a nostrconnect://"),
        (format!("nostrconnect://{}?relay={}", key, urlencode(&r)), "no secret"),
        (format!("nostrconnect://{}?secret=s", key), "no relay"),
        (format!("nostrconnect://{}?relay=ws%3A%2F%2Frelay.example.com&secret=s", key), "unencrypted ws://"),
        (format!("nostrconnect://{}?relay={}&secret=s", app.bunker.public_key().to_hex(), urlencode(&r)), "own identities"),
    ] {
        for res in [preview(&app, &admin, &code).await, connect(&app, &admin, json!({ "uri": code, "kinds": [1] })).await] {
            let (status, error, message) = error_of(res).await;
            assert_eq!((status, error.as_str()), (StatusCode::BAD_REQUEST, "invalid_nostrconnect_uri"), "{}: {}", code, message);
            assert!(message.contains(expected), "{}: {}", code, message);
        }
    }

    // Nothing the member chose is something the app asked for.
    let code = format!("nostrconnect://{}?relay={}&secret=s&perms=sign_event:1", key, urlencode(&r));
    let (status, error, _) = error_of(connect(&app, &admin, json!({ "uri": code, "kinds": [7] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::BAD_REQUEST, "nothing_grantable"));

    // A key that is already connected is refused, and the preview says so.
    assert_eq!(connect(&app, &admin, json!({ "uri": code, "kinds": [1] })).await.status(), StatusCode::CREATED);
    let shown: NostrConnectPreview = preview(&app, &admin, &code).await.json().await.unwrap();
    assert!(shown.already_connected);
    assert!(shown.relays[0].bunker);
    let (status, error, _) = error_of(connect(&app, &admin, json!({ "uri": code, "kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::CONFLICT, "already_connected"));
}

#[tokio::test]
async fn connecting_from_a_code_is_refused_with_nip46_off() {
    let app = common::spawn(true).await;
    let code = format!("nostrconnect://{}?relay=wss%3A%2F%2Frelay.example.com&secret=s", Keys::generate().public_key().to_hex());
    let (status, error, _) = error_of(connect(&app, &app.admin.public_key().to_hex(), json!({ "uri": code, "kinds": [1] })).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::CONFLICT, "nip46_disabled"));
}

#[tokio::test]
async fn rust_nostr_0_39s_older_code_without_a_secret_is_refused_and_says_why() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example.com".to_string()])).await;
    let uri = NostrConnectURI::client(Keys::generate().public_key(), [RelayUrl::parse("wss://relay.example.com").unwrap()], "rust-nostr app");
    let (status, error, message) = error_of(preview(&app, &app.admin.public_key().to_hex(), &uri.to_string()).await).await;
    assert_eq!((status, error.as_str()), (StatusCode::BAD_REQUEST, "invalid_nostrconnect_uri"));
    assert!(message.contains("older form"), "{}", message);
}

/// nostr-tools itself (the frontend's copy) against the bunker, through a local relay. Needs Node
/// and `frontend/node_modules`, so it is run by hand:
/// `cargo test --test nostrconnect_test -- --ignored`. The frontend's end-to-end test covers the
/// same through the console.
#[tokio::test]
#[ignore]
async fn nostr_tools_bunker_signer_from_uri_connects_switches_relays_and_signs() {
    let bunker_relay = ports::run_mock_relay().await;
    let app_relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![bunker_relay.url()])).await;
    start_bunker(&app, &bunker_relay.url()).await;

    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/nostr-tools-nostrconnect.mjs");
    let mut child = tokio::process::Command::new("node")
        .arg(&script)
        .arg(app_relay.url())
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("node");
    let stdout = child.stdout.take().unwrap();
    let mut lines = tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout));
    let code = lines.next_line().await.unwrap().expect("the code nostr-tools made");
    // nostr-tools subscribes once it has printed the code, and the answer is ephemeral: give it the
    // moment a person pasting the code would.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let res = connect(&app, &app.admin.public_key().to_hex(), json!({ "uri": code, "kinds": [1] })).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let mut report = String::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        report.push_str(&line);
        report.push('\n');
    }
    assert!(child.wait().await.unwrap().success(), "{}", report);
    let report: Value = serde_json::from_str(report.trim()).unwrap_or_else(|_| panic!("{}", report));
    assert_eq!(report["pubkey"], app.bunker.public_key().to_hex());
    assert_eq!(report["relays"], json!([bunker_relay.url()]));
    assert_eq!(report["kind1"], true);
    assert_eq!(report["kind7"], "refused");
}
