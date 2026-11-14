// NIP-46 connections end to end (#52, #53): an administrator issues a token through the API, a
// standard NIP-46 client (rust-nostr's nostr-connect) connects with it through a relay, learns the
// bunker's key and signs a granted kind; a kind outside the grant is refused; the connection is
// listed for its member and survives a restart. Plus who may issue, list and revoke, and the
// encryption methods (#97) over the same client, and connection lifetimes (#98).
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
    let user = app.register(Role::Signer);
    start_bunker_relay_client(&app, &relay.url()).await;

    // An administrator issues a token for the signer, allowing notes and reactions.
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
    let user = app.register(Role::Signer).public_key().to_hex();
    let viewer = app.register(Role::Viewer).public_key().to_hex();
    let admin = app.admin.public_key().to_hex();

    for who in [&user, &viewer] {
        assert_eq!(issue(&app, who, json!({ "label": "x", "kinds": [1] })).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(app.signed(Method::GET, "/api/bunker/connections/tokens", who).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    }
    // Revoking is open to signers since #31, scoped to their own: an unknown id is a 404. A viewer
    // holds no connections, so the route is refused (#77).
    assert_eq!(app.signed(Method::DELETE, "/api/bunker/connections/some-id", &user).send().await.unwrap().status(), StatusCode::NOT_FOUND);
    assert_eq!(app.signed(Method::DELETE, "/api/bunker/connections/some-id", &viewer).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(app.signed(Method::GET, "/api/bunker/connections", &viewer).send().await.unwrap().status(), StatusCode::FORBIDDEN);

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
    let alice = app.register(Role::Signer);
    let bob = app.register(Role::Signer);
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
        (json!({ "label": "x" }), "invalid_kinds"),
        (json!({ "label": "x", "kinds": [], "methods": [] }), "invalid_kinds"),
        (json!({ "label": "x", "kinds": [1], "methods": ["nip44_sign"] }), "invalid_methods"),
        (json!({ "label": "x", "methods": ["sign_event:1"] }), "invalid_methods"),
        (json!({ "label": "x", "kinds": [1], "connection_lifetime_hours": 0 }), "invalid_lifetime"),
        (json!({ "label": "x", "kinds": [1], "connection_lifetime_hours": 8761 }), "invalid_lifetime"),
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

#[tokio::test]
async fn a_standard_nip46_client_encrypts_and_decrypts_only_with_the_methods_granted() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    let user = app.register(Role::Signer);
    start_bunker_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();

    // Methods without kinds, listed back in their fixed order, without duplicates.
    let res = issue(&app, &admin, json!({ "for_pubkey": user.public_key().to_hex(), "label": "DMs", "methods": ["nip44_decrypt", "nip44_encrypt", "nip44_decrypt"] })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert!(token.kinds.is_empty());
    assert_eq!(token.methods, vec!["nip44_encrypt", "nip44_decrypt"]);
    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(tokens[0].methods, vec!["nip44_encrypt", "nip44_decrypt"]);

    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    let bunker = client.get_public_key().await.unwrap();
    let friend = Keys::generate();

    // NIP-44 both ways, with a third party, as a NIP-17 client does.
    let sent = client.nip44_encrypt(&friend.public_key(), "hello from the brand").await.unwrap();
    assert_eq!(nip44::decrypt(friend.secret_key(), &bunker, &sent).unwrap(), "hello from the brand");
    let incoming = nip44::encrypt(friend.secret_key(), &bunker, "hello brand", nip44::Version::V2).unwrap();
    assert_eq!(client.nip44_decrypt(&friend.public_key(), &incoming).await.unwrap(), "hello brand");

    // NIP-04 and signing were not granted.
    assert!(client.nip04_encrypt(&friend.public_key(), "x").await.is_err());
    assert!(client.nip04_decrypt(&friend.public_key(), &nip04::encrypt(friend.secret_key(), &bunker, "x").unwrap()).await.is_err());
    assert!(client.sign_event(EventBuilder::text_note("not granted").build(bunker)).await.is_err());

    // The connection lists its methods, and the one decrypt is in the audit log.
    let listed = connections(&app, &admin).await;
    assert_eq!(listed[0].methods, vec!["nip44_encrypt", "nip44_decrypt"]);
    let decrypts: Vec<_> = app.db.audit_events().unwrap().into_iter().filter(|e| e.action.ends_with("_decrypt")).collect();
    assert_eq!(decrypts.len(), 1);
    assert_eq!(decrypts[0].action, "nip44_decrypt");
    assert_eq!(decrypts[0].member_pubkey.as_deref(), Some(user.public_key().to_hex().as_str()));
    assert_eq!(decrypts[0].detail.as_deref(), Some(format!("peer={}", friend.public_key().to_hex()).as_str()));
}

#[tokio::test]
async fn a_standard_nip46_client_encrypts_and_decrypts_nip04() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let token: IssueTokenResponse = issue(&app, &app.admin.public_key().to_hex(), json!({ "label": "Old app", "kinds": [4], "methods": ["nip04_encrypt", "nip04_decrypt"] })).await.json().await.unwrap();
    assert_eq!(token.methods, vec!["nip04_encrypt", "nip04_decrypt"]);

    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    let bunker = client.get_public_key().await.unwrap();
    let friend = Keys::generate();
    let sent = client.nip04_encrypt(&friend.public_key(), "hello").await.unwrap();
    assert_eq!(nip04::decrypt(friend.secret_key(), &bunker, &sent).unwrap(), "hello");
    let incoming = nip04::encrypt(friend.secret_key(), &bunker, "hi").unwrap();
    assert_eq!(client.nip04_decrypt(&friend.public_key(), &incoming).await.unwrap(), "hi");
    assert!(client.nip44_encrypt(&friend.public_key(), "x").await.is_err());
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

#[tokio::test]
async fn a_standard_nip46_client_on_a_connection_with_a_lifetime_signs_and_is_shown_its_end() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();

    // Issued for a week; the token itself can still be used for only 24 hours.
    let res = issue(&app, &admin, json!({ "label": "Contractor", "kinds": [1], "connection_lifetime_hours": 168 })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(token.connection_lifetime_hours, Some(168));
    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(tokens[0].connection_lifetime_hours, Some(168));

    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    let bunker = client.get_public_key().await.unwrap();
    assert!(client.sign_event(EventBuilder::text_note("within its time").build(bunker)).await.is_ok());

    // The week runs from when it connected.
    let listed = connections(&app, &admin).await;
    let connected = chrono::DateTime::parse_from_rfc3339(&listed[0].connected_at).unwrap();
    let ends = chrono::DateTime::parse_from_rfc3339(listed[0].expires_at.as_deref().expect("an end")).unwrap();
    // Stored to the microsecond, so within a second of 168 hours.
    assert!(((ends - connected) - chrono::Duration::hours(168)).num_seconds().abs() < 1, "{} to {}", connected, ends);

    // Without a lifetime, as before: no end.
    let open: IssueTokenResponse = issue(&app, &admin, json!({ "label": "Phone", "kinds": [1] })).await.json().await.unwrap();
    assert_eq!(open.connection_lifetime_hours, None);
}

#[tokio::test]
async fn an_expired_connection_is_refused_over_the_relay_and_listed_as_expired() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();

    // A connection made two hours ago for one hour, and one made then for a day.
    let then = chrono::Utc::now() - chrono::Duration::hours(2);
    let connect_then = |lifetime: u32| {
        let secret = format!("secret-{}", uuid::Uuid::new_v4());
        app.db
            .create_nip46_token_with_lifetime(&bunker::nip46::hash_secret(&secret), &admin, &admin, "x", &["sign_event:1".to_string()], then, then + chrono::Duration::hours(24), Some(lifetime))
            .unwrap();
        let keys = Keys::generate();
        app.db.redeem_nip46_token(&bunker::nip46::hash_secret(&secret), &keys.public_key().to_hex(), None, &Default::default(), then).unwrap().unwrap();
        keys
    };
    let expired = connect_then(1);
    let current = connect_then(24);

    let bunker = app.bunker.public_key();
    let note = |keys: &Keys| serde_json::to_string(&EventBuilder::text_note("hello").build(keys.public_key())).unwrap();
    for (keys, expected) in [(&current, None), (&expired, Some(bunker::nip46::EXPIRED)), (&expired, Some(bunker::nip46::NOT_CONNECTED))] {
        let client = Client::builder().signer(keys.clone()).build();
        client.add_relay(relay.url()).await.unwrap();
        client.connect().await;
        client.subscribe(Filter::new().kind(Kind::from(24133)).pubkey(keys.public_key()), None).await.unwrap();
        let answer = nip44_request(&client, keys, bunker, "sign_event", vec![note(keys)]).await;
        match expected {
            None => assert!(answer["result"].is_string(), "{}", answer),
            Some(error) => assert_eq!(answer["error"], error, "{}", answer),
        }
    }

    let listed = connections(&app, &admin).await;
    let ended = listed.iter().find(|c| c.client_pubkey == expired.public_key().to_hex()).unwrap();
    assert_eq!(ended.revoked_reason.as_deref(), Some("expired"));
    assert_eq!(ended.revoked_at, ended.expires_at);
    assert!(listed.iter().find(|c| c.client_pubkey == current.public_key().to_hex()).unwrap().revoked_at.is_none());
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
    let alice = app.register(Role::Signer);
    let bob = app.register(Role::Signer);
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
    let alice = app.register(Role::Signer);
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

#[tokio::test]
async fn a_token_is_not_issued_for_a_viewer() {
    // Viewers cannot sign (#77): issuing for one is refused, for an administrator or a signer not.
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();
    let viewer = app.register(Role::Viewer).public_key().to_hex();
    let signer = app.register(Role::Signer).public_key().to_hex();

    // Nor with the encryption methods alone (#97): a viewer can never hold them.
    for body in [
        json!({ "for_pubkey": viewer, "label": "x", "kinds": [1] }),
        json!({ "for_pubkey": viewer, "label": "x", "methods": ["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"] }),
    ] {
        let res = issue(&app, &admin, body).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(res.json::<Value>().await.unwrap()["error"], "member_cannot_sign");
    }
    assert!(app.db.list_nip46_tokens().unwrap().is_empty(), "nothing was issued");

    for who in [&signer, &admin] {
        let res = issue(&app, &admin, json!({ "for_pubkey": who, "label": "x", "kinds": [1] })).await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn a_member_who_becomes_a_viewer_loses_their_connections_and_stops_signing() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();
    let alice = app.register(Role::Signer);
    let alice_hex = alice.public_key().to_hex();
    let client = connect_for(&app, &alice).await;
    assert!(sign_as(&app, &client).await.result.is_some());
    // A token issued for her but not yet used.
    let pending: IssueTokenResponse =
        issue(&app, &admin, json!({ "for_pubkey": alice_hex, "label": "later", "kinds": [1] })).await.json().await.unwrap();
    let pending_secret = pending.uri.split("secret=").nth(1).unwrap().to_string();

    assert_eq!(app.db.change_member_role(&alice_hex, Role::Viewer).unwrap(), bunker::db::RoleChangeOutcome::Changed);

    // Revoked, with the reason, and refused as a removed member's would be.
    let listed = connections(&app, &admin).await;
    let hers = listed.iter().find(|c| c.for_pubkey == alice_hex).unwrap();
    assert!(hers.revoked_at.is_some());
    assert_eq!(hers.revoked_reason.as_deref(), Some("role_changed"));
    assert_eq!(sign_as(&app, &client).await.error.as_deref(), Some(bunker::nip46::NOT_CONNECTED));
    // Her unused token went with it.
    let connect = app
        .state
        .nip46_handler
        .handle_request(
            bunker::nip46::Nip46Request { id: "c".into(), method: "connect".into(), params: vec![app.bunker.public_key().to_hex(), pending_secret] },
            Keys::generate().public_key(),
        )
        .await;
    assert!(connect.result.is_none());
    let metrics: Value = app.get("/api/bunker/metrics").send().await.unwrap().json().await.unwrap();
    assert_eq!(metrics["nip46_connections"], 0);
}

#[tokio::test]
async fn a_token_for_a_viewer_does_not_connect() {
    // The defence behind the API's refusal (#77): redeeming checks the member's role as it is now,
    // so a token stored for a viewer by any other path still connects nobody.
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let viewer = app.register(Role::Viewer).public_key().to_hex();
    let now = chrono::Utc::now();
    app.db
        .create_nip46_token(&bunker::nip46::hash_secret("s3cret"), &viewer, "", "x", &["sign_event:1".to_string()], now, now + chrono::Duration::hours(1))
        .unwrap();

    let refusal = app
        .db
        .redeem_nip46_token(&bunker::nip46::hash_secret("s3cret"), &Keys::generate().public_key().to_hex(), None, &Default::default(), now)
        .unwrap();
    assert_eq!(refusal.err(), Some(bunker::db::RedeemRefusal::MemberCannotSign));
}
