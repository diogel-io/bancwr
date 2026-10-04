// NIP-46 connections end to end (#52, #53): an administrator issues a token through the API, a
// standard NIP-46 client (rust-nostr's nostr-connect) connects with it through a relay, learns the
// bunker's key and signs a granted kind; a kind outside the grant is refused; the connection is
// listed for its member and survives a restart. Plus who may issue, list and revoke.
#[path = "../common/mod.rs"]
mod common;

use bunker::connections_api::{ConnectionResponse, IssueTokenResponse, TokenResponse};
use bunker::registry::Role;
use bunker::relay::RelayClient;
use nostr_connect::prelude::*;
use nostr_sdk::{Client, RelayPoolNotification};
use nostr_relay_builder::MockRelay;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

async fn issue(app: &common::TestApp, as_identity: &str, body: Value) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/tokens", as_identity).json(&body).send().await.unwrap()
}

async fn connections(app: &common::TestApp, as_identity: &str) -> Vec<ConnectionResponse> {
    let res = app.signed(Method::GET, "/api/bunker/connections", as_identity).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

/// The bunker's NIP-46 relay client, started on the app's state, as main.rs starts it.
async fn start_bunker_relay_client(app: &common::TestApp, relay: &str) {
    let client = RelayClient::new(vec![relay.to_string()], app.state.clone()).await.unwrap();
    tokio::spawn(async move {
        let _ = client.run().await;
    });
    // Let it connect and subscribe before a client sends anything.
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test]
async fn a_standard_nip46_client_connects_with_an_issued_token_and_signs_only_what_was_granted() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    let user = app.register(Role::User);
    start_bunker_relay_client(&app, &relay.url()).await;

    // An administrator issues a token for the user, allowing notes and reactions.
    let res = issue(&app, &app.admin.public_key().to_hex(), json!({ "for_pubkey": user.public_key().to_bech32().unwrap(), "label": "Test client", "kinds": [1, 7] })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert!(token.uri.starts_with(&format!("bunker://{}?relay=", app.bunker.public_key().to_hex())));
    assert_eq!(token.kinds, vec![1, 7]);

    // A standard client, with only the bunker:// string.
    let uri = NostrConnectURI::parse(&token.uri).unwrap();
    let client = NostrConnect::new(uri, Keys::generate(), Duration::from_secs(10), None).unwrap();
    let key = client.get_public_key().await;
    assert_eq!(key.as_ref().map_err(|e| e.to_string()), Ok(&app.bunker.public_key()));

    let note = client.sign_event(EventBuilder::text_note("hello from a NIP-46 client").build(app.bunker.public_key())).await.unwrap();
    assert_eq!(note.pubkey, app.bunker.public_key());
    assert!(note.verify().is_ok());

    let profile = client.sign_event(EventBuilder::metadata(&Metadata::new().name("not granted")).build(app.bunker.public_key())).await;
    assert!(profile.is_err(), "kind 0 is outside the grant");

    // Listed for its member, as unverified, with the kinds granted and a last use.
    let listed = connections(&app, &user.public_key().to_hex()).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].for_pubkey, user.public_key().to_hex());
    assert_eq!(listed[0].kinds, vec![1, 7]);
    assert!(!listed[0].metadata_verified);
    assert!(listed[0].last_used_at.is_some());

    // The token cannot connect a second client.
    let again = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    assert!(again.get_public_key().await.is_err());
}

#[tokio::test]
async fn a_connection_survives_a_bunker_restart() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let token: IssueTokenResponse = issue(&app, &app.admin.public_key().to_hex(), json!({ "label": "Phone", "kinds": [1] })).await.json().await.unwrap();
    let app_keys = Keys::generate();
    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), app_keys.clone(), Duration::from_secs(10), None).unwrap();
    client.get_public_key().await.unwrap();

    // The handler reads connections from the database, so a new one on the same database (as a
    // restarted bunker has) still knows the client.
    let restarted = bunker::nip46::Nip46Handler::new(app.state.signer.clone(), app.db.clone());
    let unsigned = EventBuilder::text_note("after restart").build(app.bunker.public_key());
    let response = restarted
        .handle_request(
            bunker::nip46::Nip46Request { id: "1".into(), method: "sign_event".into(), params: vec![serde_json::to_string(&unsigned).unwrap()] },
            app_keys.public_key(),
        )
        .await;
    assert!(response.result.is_some(), "{:?}", response.error);
}

#[tokio::test]
async fn only_administrators_issue_list_tokens_and_revoke() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let user = app.register(Role::User).public_key().to_hex();
    let signer = app.register(Role::Signer).public_key().to_hex();
    let admin = app.admin.public_key().to_hex();

    for who in [&user, &signer] {
        assert_eq!(issue(&app, who, json!({ "label": "x", "kinds": [1] })).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(app.signed(Method::GET, "/api/bunker/connections/tokens", who).send().await.unwrap().status(), StatusCode::FORBIDDEN);
        // Revoking is open to every member since #31, scoped to their own: an unknown id is a 404.
        assert_eq!(app.signed(Method::DELETE, "/api/bunker/connections/some-id", who).send().await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    let issued: IssueTokenResponse = issue(&app, &admin, json!({ "for_pubkey": user, "label": "Laptop", "kinds": [7, 1, 1] })).await.json().await.unwrap();
    let secret = issued.uri.split("secret=").nth(1).unwrap().to_string();
    assert_eq!(issued.kinds, vec![1, 7]);

    // The secret appears only in the creation response.
    let res = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap();
    let body = res.text().await.unwrap();
    assert!(!body.contains(&secret));
    let tokens: Vec<TokenResponse> = serde_json::from_str(&body).unwrap();
    assert_eq!(tokens[0].label, "Laptop");
    assert_eq!(tokens[0].issued_by, admin);

    let revoked = app.signed(Method::DELETE, &format!("/api/bunker/connections/tokens/{}", issued.id), &admin).send().await.unwrap();
    assert_eq!(revoked.status(), StatusCode::OK);
    let again = app.signed(Method::DELETE, &format!("/api/bunker/connections/tokens/{}", issued.id), &admin).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn each_member_sees_only_their_own_connections_and_an_administrator_sees_all() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let alice = app.register(Role::User);
    let bob = app.register(Role::User);
    let admin = app.admin.public_key().to_hex();

    // Connect one client for each, through the handler.
    for member in [&alice, &bob] {
        let issued: IssueTokenResponse = issue(&app, &admin, json!({ "for_pubkey": member.public_key().to_hex(), "label": "x", "kinds": [1] })).await.json().await.unwrap();
        let secret = issued.uri.split("secret=").nth(1).unwrap().to_string();
        let response = app
            .state
            .nip46_handler
            .handle_request(
                bunker::nip46::Nip46Request { id: "c".into(), method: "connect".into(), params: vec![app.bunker.public_key().to_hex(), secret] },
                Keys::generate().public_key(),
            )
            .await;
        assert_eq!(response.result.as_deref(), Some("ack"));
    }

    let mine = connections(&app, &alice.public_key().to_hex()).await;
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].for_pubkey, alice.public_key().to_hex());
    assert_eq!(connections(&app, &admin).await.len(), 2);

    // An administrator revokes one, which then shows as revoked.
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", mine[0].id), &admin).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(connections(&app, &alice.public_key().to_hex()).await[0].revoked_reason.as_deref(), Some("revoked"));
}

#[tokio::test]
async fn issuing_is_validated_and_refused_with_nip46_off() {
    let off = common::spawn(true).await;
    let res = issue(&off, &off.admin.public_key().to_hex(), json!({ "label": "x", "kinds": [1] })).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.json::<Value>().await.unwrap()["error"], "nip46_disabled");

    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();
    let stranger = Keys::generate().public_key().to_hex();
    for (body, error) in [
        (json!({ "label": "", "kinds": [1] }), "invalid_label"),
        (json!({ "label": "x", "kinds": [] }), "invalid_kinds"),
        (json!({ "label": "x", "kinds": [70000] }), "invalid_kinds"),
        (json!({ "label": "x", "kinds": [1], "expires_in_hours": 0 }), "invalid_expiry"),
        (json!({ "label": "x", "kinds": [1], "expires_in_hours": 169 }), "invalid_expiry"),
        (json!({ "label": "x", "kinds": [1], "for_pubkey": "nope" }), "invalid_pubkey"),
        (json!({ "label": "x", "kinds": [1], "for_pubkey": stranger }), "not_registered"),
    ] {
        let res = issue(&app, &admin, body.clone()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{}", body);
        assert_eq!(res.json::<Value>().await.unwrap()["error"], error, "{}", body);
    }
}

/// A NIP-44 request, as nostr-tools' BunkerSigner sends (#31's frontend client), answered in NIP-44
/// with kind 24133.
async fn nip44_request(client: &Client, keys: &Keys, bunker: PublicKey, method: &str, params: Vec<String>) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    let body = json!({ "id": id, "method": method, "params": params }).to_string();
    let content = nip44::encrypt(keys.secret_key(), &bunker, body, nip44::Version::V2).unwrap();
    let mut notifications = client.notifications();
    let request = EventBuilder::new(Kind::from(24133), content).tag(Tag::public_key(bunker)).sign_with_keys(keys).unwrap();
    client.send_event(request).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Ok(notification) = notifications.recv().await {
            let RelayPoolNotification::Event { event, .. } = notification else { continue };
            if event.kind == Kind::from(24133) && event.pubkey == bunker {
                let text = nip44::decrypt(keys.secret_key(), &bunker, &event.content).expect("a NIP-44 answer");
                let response: Value = serde_json::from_str(&text).unwrap();
                if response["id"] == id {
                    return response;
                }
            }
        }
        panic!("notifications ended")
    })
    .await
    .expect("an answer within 10 s")
}

#[tokio::test]
async fn a_nip44_client_is_answered_in_nip44_with_kind_24133() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let token: IssueTokenResponse = issue(&app, &app.admin.public_key().to_hex(), json!({ "label": "nostr-tools", "kinds": [1] })).await.json().await.unwrap();
    let secret = token.uri.split("secret=").nth(1).unwrap().to_string();

    let keys = Keys::generate();
    let client = Client::builder().signer(keys.clone()).build();
    client.add_relay(relay.url()).await.unwrap();
    client.connect().await;
    client.subscribe(Filter::new().kind(Kind::from(24133)).pubkey(keys.public_key()), None).await.unwrap();

    let bunker = app.bunker.public_key();
    let connected = nip44_request(&client, &keys, bunker, "connect", vec![bunker.to_hex(), secret]).await;
    assert_eq!(connected["result"], "ack", "{}", connected);
    let key = nip44_request(&client, &keys, bunker, "get_public_key", vec![]).await;
    assert_eq!(key["result"], bunker.to_hex());
}

/// Connects one client for `member` through the handler; returns the client keys.
async fn connect_for(app: &common::TestApp, member: &Keys) -> Keys {
    let admin = app.admin.public_key().to_hex();
    let issued: IssueTokenResponse = issue(app, &admin, json!({ "for_pubkey": member.public_key().to_hex(), "label": "x", "kinds": [1] })).await.json().await.unwrap();
    let secret = issued.uri.split("secret=").nth(1).unwrap().to_string();
    let client = Keys::generate();
    let response = app
        .state
        .nip46_handler
        .handle_request(
            bunker::nip46::Nip46Request { id: "c".into(), method: "connect".into(), params: vec![app.bunker.public_key().to_hex(), secret] },
            client.public_key(),
        )
        .await;
    assert_eq!(response.result.as_deref(), Some("ack"));
    client
}

async fn sign_as(app: &common::TestApp, client: &Keys) -> bunker::nip46::Nip46Response {
    let unsigned = EventBuilder::text_note("x").build(app.bunker.public_key());
    app.state
        .nip46_handler
        .handle_request(
            bunker::nip46::Nip46Request { id: "s".into(), method: "sign_event".into(), params: vec![serde_json::to_string(&unsigned).unwrap()] },
            client.public_key(),
        )
        .await
}

#[tokio::test]
async fn a_member_revokes_their_own_connection_but_not_anyone_elses() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let alice = app.register(Role::User);
    let bob = app.register(Role::User);
    let alice_client = connect_for(&app, &alice).await;
    let bob_client = connect_for(&app, &bob).await;
    let alice_hex = alice.public_key().to_hex();

    let bobs = connections(&app, &bob.public_key().to_hex()).await;
    // Alice cannot revoke Bob's: a 404, as for an id that does not exist.
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", bobs[0].id), &alice_hex).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(sign_as(&app, &bob_client).await.result.is_some());

    // Alice revokes her own; it records her, and the client can sign no more.
    let mine = connections(&app, &alice_hex).await;
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", mine[0].id), &alice_hex).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let after = connections(&app, &alice_hex).await;
    assert_eq!(after[0].revoked_reason.as_deref(), Some("revoked"));
    assert_eq!(after[0].revoked_by.as_deref(), Some(alice_hex.as_str()));
    assert_eq!(sign_as(&app, &alice_client).await.error.as_deref(), Some(bunker::nip46::NOT_CONNECTED));

    // An administrator revokes anyone's, and is recorded.
    let admin = app.admin.public_key().to_hex();
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", bobs[0].id), &admin).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(connections(&app, &admin).await.iter().find(|c| c.id == bobs[0].id).unwrap().revoked_by.as_deref(), Some(admin.as_str()));
}

#[tokio::test]
async fn a_signer_may_revoke_connections_made_for_them() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let signer = app.register(Role::Signer);
    connect_for(&app, &signer).await;
    let hex = signer.public_key().to_hex();
    let mine = connections(&app, &hex).await;
    assert_eq!(mine.len(), 1);
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/{}", mine[0].id), &hex).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_log_names_the_member_a_signature_was_for_not_the_app() {
    // diogel-io/workspace#38: the log used to show the client's key as the member.
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let alice = app.register(Role::User);
    let client = connect_for(&app, &alice).await;
    assert!(sign_as(&app, &client).await.result.is_some());

    let connection_id = connections(&app, &alice.public_key().to_hex()).await[0].id.clone();
    let logs: Vec<Value> = app.get("/api/bunker/logs").send().await.unwrap().json().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["pubkey"], client.public_key().to_hex());
    assert_eq!(logs[0]["member_pubkey"], alice.public_key().to_hex());
    assert_eq!(logs[0]["member_name"], "Member");
    assert_eq!(logs[0]["connection_id"], connection_id);
}
