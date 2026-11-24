// NIP-46 connections end to end (#52, #53): an administrator issues a token through the API, a
// standard NIP-46 client (rust-nostr's nostr-connect) connects with it through a relay, learns the
// bunker's key and signs a granted kind; a kind outside the grant is refused; the connection is
// listed for its member and survives a restart. Plus who may issue, list and revoke (a signer
// only their own, #142), and the
// encryption methods (#97) over the same client, connection lifetimes (#98), rate limits (#101), and
// approval before signing (#102): approved, rejected and timed out, over the relay.
#[path = "../common/mod.rs"]
mod common;
#[path = "../support/ports.rs"]
mod ports;

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
    let relay = ports::run_mock_relay().await;
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
    let relay = ports::run_mock_relay().await;
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
async fn administrators_and_signers_issue_list_and_revoke_tokens_but_viewers_do_not() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let user = app.register(Role::Signer).public_key().to_hex();
    let viewer = app.register(Role::Viewer).public_key().to_hex();
    let admin = app.admin.public_key().to_hex();

    // A viewer holds no connections (#77): every token route and the connection routes are refused.
    assert_eq!(issue(&app, &viewer, json!({ "label": "x", "kinds": [1] })).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.signed(Method::GET, "/api/bunker/connections/tokens", &viewer).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(app.signed(Method::DELETE, "/api/bunker/connections/tokens/some-id", &viewer).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    // Revoking is open to signers since #31, scoped to their own: an unknown id is a 404.
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

async fn tokens(app: &common::TestApp, as_identity: &str) -> Vec<TokenResponse> {
    let res = app.signed(Method::GET, "/api/bunker/connections/tokens", as_identity).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

#[tokio::test]
async fn a_signer_issues_lists_and_revokes_only_their_own_tokens() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();
    let signer = app.register(Role::Signer).public_key().to_hex();
    let other = app.register(Role::Signer).public_key().to_hex();

    // For themselves, with for_pubkey omitted or naming their own key (npub or hex).
    let res = issue(&app, &signer, json!({ "label": "Phone", "kinds": [1], "methods": ["nip44_encrypt"], "expires_in_hours": 1 })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let own: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(own.for_pubkey, signer);
    assert!(own.uri.starts_with(&format!("bunker://{}?relay=", app.bunker.public_key().to_hex())));
    let npub = PublicKey::parse(&signer).unwrap().to_bech32().unwrap();
    assert_eq!(issue(&app, &signer, json!({ "for_pubkey": npub, "label": "Tablet", "kinds": [7] })).await.status(), StatusCode::OK);

    // Never for another member: refused, and nothing is created.
    let before = app.db.list_nip46_tokens().unwrap().len();
    let refused = issue(&app, &signer, json!({ "for_pubkey": other, "label": "Theirs", "kinds": [1] })).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "forbidden_member");
    let refused = issue(&app, &signer, json!({ "for_pubkey": admin, "label": "Admin's", "kinds": [1] })).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    // Approval is an administrator's control (#102).
    let refused = issue(&app, &signer, json!({ "label": "Held", "kinds": [1], "approval_kinds": [1] })).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "approval_admin_only");
    // The same bounds as an administrator's token.
    assert_eq!(issue(&app, &signer, json!({ "label": "Long", "kinds": [1], "expires_in_hours": 169 })).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(app.db.list_nip46_tokens().unwrap().len(), before, "nothing was issued");

    // The issuer is recorded as the signer.
    let theirs: IssueTokenResponse = issue(&app, &admin, json!({ "for_pubkey": other, "label": "Other's laptop", "kinds": [1] })).await.json().await.unwrap();
    let listed = tokens(&app, &signer).await;
    assert_eq!(listed.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["Tablet", "Phone"]);
    assert!(listed.iter().all(|t| t.for_pubkey == signer && t.issued_by == signer));
    assert_eq!(tokens(&app, &admin).await.len(), 3, "an administrator sees every token");

    // Another member's token is a 404, as an unknown one is, and stays unrevoked.
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/tokens/{}", theirs.id), &signer).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(tokens(&app, &other).await.iter().all(|t| t.revoked_at.is_none()));
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/tokens/{}", own.id), &signer).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(tokens(&app, &signer).await.iter().find(|t| t.id == own.id).unwrap().revoked_at.is_some());
    // An administrator still revokes any.
    let res = app.signed(Method::DELETE, &format!("/api/bunker/connections/tokens/{}", theirs.id), &admin).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
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
        (json!({ "label": "x", "kinds": [1], "rate_per_minute": 0 }), "invalid_rate_limit"),
        (json!({ "label": "x", "kinds": [1], "approval_kinds": [7] }), "invalid_approval_kinds"),
        (json!({ "label": "x", "kinds": [1], "approval_kinds": [70000] }), "invalid_approval_kinds"),
        (json!({ "label": "x", "kinds": [1], "rate_per_minute": 601 }), "invalid_rate_limit"),
        (json!({ "label": "x", "kinds": [1], "rate_burst": 0 }), "invalid_rate_limit"),
        (json!({ "label": "x", "kinds": [1], "rate_burst": 101 }), "invalid_rate_limit"),
        (json!({ "label": "x", "kinds": [1], "rate_per_minute": 10, "rate_burst": 500 }), "invalid_rate_limit"),
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
    let relay = ports::run_mock_relay().await;
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
    let relay = ports::run_mock_relay().await;
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
    let relay = ports::run_mock_relay().await;
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

/// A raw client on `relay` listening for answers to `keys`.
async fn raw_client(relay: &str, keys: &Keys) -> Client {
    let client = Client::builder().signer(keys.clone()).build();
    client.add_relay(relay).await.unwrap();
    client.connect().await;
    client.subscribe(Filter::new().kind(Kind::from(24133)).pubkey(keys.public_key()), None).await.unwrap();
    client
}

#[tokio::test]
async fn over_the_relay_a_refused_connect_uses_up_its_token_and_no_sender_can_be_decrypted() {
    let relay = MockRelay::run().await.unwrap();
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();
    let bunker = app.bunker.public_key();

    // A connected app that may decrypt.
    let reader: IssueTokenResponse = issue(&app, &admin, json!({ "label": "Reader", "methods": ["nip44_decrypt", "nip04_decrypt"] })).await.json().await.unwrap();
    let reader = NostrConnect::new(NostrConnectURI::parse(&reader.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    reader.get_public_key().await.unwrap();

    // Another app connects asking only for a kind its token does not allow, and is refused.
    let token: IssueTokenResponse = issue(&app, &admin, json!({ "label": "Phone", "kinds": [1] })).await.json().await.unwrap();
    let secret = token.uri.split("secret=").nth(1).unwrap().to_string();
    let phone = Keys::generate();
    let phone_client = raw_client(&relay.url(), &phone).await;
    let body = json!({ "id": "c", "method": "connect", "params": [bunker.to_hex(), secret, "sign_event:4"] }).to_string();
    let refused = nip44_request(&phone_client, &phone, bunker, "connect", vec![bunker.to_hex(), secret.clone(), "sign_event:4".into()]).await;
    assert!(refused["error"].as_str().unwrap().ends_with(bunker::nip46::TOKEN_USED_UP), "{}", refused);

    // A key whose request the bunker could read but not parse.
    let garbled = Keys::generate();
    let content = nip44::encrypt(garbled.secret_key(), &bunker, r#"{"id":"g","method":"connect","params":[null]}"#, nip44::Version::V2).unwrap();
    let event = EventBuilder::new(Kind::from(24133), content).tag(Tag::public_key(bunker)).sign_with_keys(&garbled).unwrap();
    raw_client(&relay.url(), &garbled).await.send_event(event).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !app.db.is_nip46_client(&garbled.public_key().to_hex()).unwrap() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("recorded within 10 s");

    // Neither key is a peer the reader may decrypt with, whatever it sent.
    for sender in [&phone, &garbled] {
        let sent = nip44::encrypt(sender.secret_key(), &bunker, &body, nip44::Version::V2).unwrap();
        assert!(reader.nip44_decrypt(&sender.public_key(), &sent).await.is_err());
        let sent = nip04::encrypt(sender.secret_key(), &bunker, &body).unwrap();
        assert!(reader.nip04_decrypt(&sender.public_key(), &sent).await.is_err());
    }
    assert!(app.db.audit_events().unwrap().iter().all(|e| !e.action.ends_with("_decrypt")), "nothing was decrypted");

    // The token is listed as used, not as waiting, and connecting with it as issued is refused.
    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap().json().await.unwrap();
    let listed = tokens.iter().find(|t| t.id == token.id).unwrap();
    assert!(listed.used_at.is_some() && listed.revoked_at.is_none());
    let again = nip44_request(&phone_client, &phone, bunker, "connect", vec![bunker.to_hex(), secret]).await;
    assert_eq!(again["error"], "This connection token has already been used", "{}", again);
    assert!(!connections(&app, &admin).await.iter().any(|c| c.client_pubkey == phone.public_key().to_hex()));
}

#[tokio::test]
async fn a_standard_nip46_client_on_a_connection_with_a_lifetime_signs_and_is_shown_its_end() {
    let relay = ports::run_mock_relay().await;
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
    let relay = ports::run_mock_relay().await;
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

/// `PATCH /api/bunker/team/:id` as `admin` (#99).
async fn change_role(app: &common::TestApp, admin: &str, member: &str, role: &str) -> reqwest::Response {
    let id = app.db.find_member_by_pubkey(member).unwrap().unwrap().id;
    app.signed(Method::PATCH, &format!("/api/bunker/team/{}", id), admin).json(&json!({ "role": role })).send().await.unwrap()
}

#[tokio::test]
async fn a_downgrade_to_viewer_from_the_team_page_revokes_connections_and_an_upgrade_keeps_them() {
    // #99: the route applies the rule of #77 and records who changed the role.
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();
    let alice = app.register(Role::Signer);
    let alice_hex = alice.public_key().to_hex();
    let bob = app.register(Role::Signer);
    let bob_hex = bob.public_key().to_hex();
    let alices_app = connect_for(&app, &alice).await;
    let bobs_app = connect_for(&app, &bob).await;
    issue(&app, &admin, json!({ "for_pubkey": alice_hex, "label": "later", "kinds": [1] })).await;

    // Bob becomes an administrator: his app keeps signing.
    assert_eq!(change_role(&app, &admin, &bob_hex, "administrator").await.status(), StatusCode::OK);
    assert!(sign_as(&app, &bobs_app).await.result.is_some());

    // Alice becomes a viewer: her app is cut off, with the reason, and her unused token goes too.
    assert_eq!(change_role(&app, &admin, &alice_hex, "viewer").await.status(), StatusCode::OK);
    assert_eq!(sign_as(&app, &alices_app).await.error.as_deref(), Some(bunker::nip46::NOT_CONNECTED));
    let listed = connections(&app, &admin).await;
    let hers = listed.iter().find(|c| c.for_pubkey == alice_hex).unwrap();
    assert_eq!(hers.revoked_reason.as_deref(), Some("role_changed"));
    assert!(listed.iter().find(|c| c.for_pubkey == bob_hex).unwrap().revoked_at.is_none());
    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap().json().await.unwrap();
    assert!(tokens.iter().find(|t| t.label == "later").unwrap().revoked_at.is_some(), "her unused token is revoked");

    // Both changes are recorded with the administrator who made them.
    let events = app.db.audit_events().unwrap();
    let changes: Vec<_> = events.iter().filter(|e| e.action == bunker::audit::action::MEMBER_ROLE_CHANGED).collect();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|e| e.actor_pubkey.as_deref() == Some(admin.as_str())));
    assert!(changes.iter().any(|e| e.detail.as_deref() == Some("from=signer;to=viewer")));
    assert!(changes.iter().any(|e| e.detail.as_deref() == Some("from=signer;to=administrator")));

    // Made a signer again, Alice's old connection stays revoked: her app needs a new token.
    assert_eq!(change_role(&app, &admin, &alice_hex, "signer").await.status(), StatusCode::OK);
    assert_eq!(sign_as(&app, &alices_app).await.error.as_deref(), Some(bunker::nip46::NOT_CONNECTED));
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

#[tokio::test]
async fn a_rate_limit_is_issued_reported_on_the_token_and_connection_and_defaults() {
    let app = common::spawn_with(true, Some(vec!["wss://relay.example".to_string()])).await;
    let admin = app.admin.public_key().to_hex();

    let res = issue(&app, &admin, json!({ "label": "Bot", "kinds": [1], "rate_per_minute": 10, "rate_burst": 5 })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let limited: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!((limited.rate_per_minute, limited.rate_burst), (10, 5));
    // Each part defaults on its own.
    let half: IssueTokenResponse = issue(&app, &admin, json!({ "label": "Half", "kinds": [1], "rate_per_minute": 120 })).await.json().await.unwrap();
    assert_eq!((half.rate_per_minute, half.rate_burst), (120, 20));
    let default: IssueTokenResponse = issue(&app, &admin, json!({ "label": "Phone", "kinds": [1] })).await.json().await.unwrap();
    assert_eq!((default.rate_per_minute, default.rate_burst), (60, 20));

    let tokens: Vec<TokenResponse> = app.signed(Method::GET, "/api/bunker/connections/tokens", &admin).send().await.unwrap().json().await.unwrap();
    let bot = tokens.iter().find(|t| t.id == limited.id).unwrap();
    assert_eq!((bot.rate_per_minute, bot.rate_burst), (10, 5));
    let phone = tokens.iter().find(|t| t.id == default.id).unwrap();
    assert_eq!((phone.rate_per_minute, phone.rate_burst), (60, 20));

    let secret = limited.uri.split("secret=").nth(1).unwrap().to_string();
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
    let listed = connections(&app, &admin).await;
    assert_eq!((listed[0].rate_per_minute, listed[0].rate_burst), (10, 5));
}

#[tokio::test]
async fn a_standard_nip46_client_past_its_burst_is_refused_over_the_relay_and_counted() {
    let relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();

    let token: IssueTokenResponse =
        issue(&app, &admin, json!({ "label": "Bot", "kinds": [1], "rate_per_minute": 1, "rate_burst": 2 })).await.json().await.unwrap();
    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    let bunker = client.get_public_key().await.unwrap();
    for n in 0..2 {
        assert!(client.sign_event(EventBuilder::text_note(format!("note {}", n)).build(bunker)).await.is_ok());
    }
    let refused = client.sign_event(EventBuilder::text_note("one too many").build(bunker)).await;
    let error = refused.expect_err("past the burst").to_string();
    assert!(error.contains("Rate limited"), "{}", error);

    let metrics: Value = app.signed(Method::GET, "/api/bunker/metrics", &admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(metrics["nip46_rate_limited"], 1);
    assert_eq!(metrics["total_signatures"], 2);
}

/// Records each `auth_url` a client is sent (#102).
#[derive(Debug, Clone, Default)]
struct SeenAuthUrls(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl AuthUrlHandler for SeenAuthUrls {
    fn on_auth_url(&self, auth_url: Url) -> BoxedFuture<'_, Result<()>> {
        let seen = self.0.clone();
        Box::pin(async move {
            seen.lock().unwrap().push(auth_url.to_string());
            Ok(())
        })
    }
}

/// A standard client connected with a token allowing kinds 1 and 7, of which 1 needs approval.
async fn gated_client(app: &common::TestApp) -> (NostrConnect, SeenAuthUrls, PublicKey) {
    let admin = app.admin.public_key().to_hex();
    let res = issue(app, &admin, json!({ "label": "Social bot", "kinds": [1, 7], "approval_kinds": [1] })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(token.approval_kinds, vec![1]);
    let mut client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(20), None).unwrap();
    let seen = SeenAuthUrls::default();
    client.auth_url_handler(seen.clone());
    let bunker = client.get_public_key().await.unwrap();
    (client, seen, bunker)
}

/// The one request waiting, once it is.
async fn waiting(app: &common::TestApp) -> bunker::approvals_api::ApprovalResponse {
    let admin = app.admin.public_key().to_hex();
    for _ in 0..50 {
        let mut listed: Vec<bunker::approvals_api::ApprovalResponse> =
            app.signed(Method::GET, "/api/bunker/approvals", &admin).send().await.unwrap().json().await.unwrap();
        if let Some(request) = listed.pop() {
            return request;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no request is waiting for approval")
}

async fn spawn_relay_app() -> (MockRelay, common::TestApp) {
    let relay = ports::run_mock_relay().await;
    let app = common::spawn_with(true, Some(vec![relay.url()])).await;
    start_bunker_relay_client(&app, &relay.url()).await;
    (relay, app)
}

#[tokio::test]
async fn a_standard_nip46_client_waits_for_approval_and_gets_the_event_once_approved() {
    let (_relay, app) = spawn_relay_app().await;
    app.state.nip46_handler.configure_approvals(chrono::Duration::minutes(30), Some("https://console.example".to_string()));
    let admin = app.admin.public_key().to_hex();
    let (client, seen, bunker) = gated_client(&app).await;

    // A reaction does not need approval.
    assert!(client.sign_event(EventBuilder::reaction_extended(EventId::all_zeros(), bunker, None, "+").build(bunker)).await.is_ok());

    let signing = tokio::spawn(async move { client.sign_event(EventBuilder::text_note("launch day").build(bunker)).await });
    let request = waiting(&app).await;
    assert_eq!((request.kind, request.content.as_str(), request.member_name.as_deref()), (1, "launch day", Some("Admin")));
    assert_eq!(app.db.signature_count().unwrap(), 1, "only the reaction so far");
    // The client was told where the request waits.
    for _ in 0..20 {
        if !seen.0.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(seen.0.lock().unwrap().clone(), vec![format!("https://console.example/approvals/{}", request.id)]);

    let res = app
        .signed(Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &admin)
        .json(&json!({ "event_id": request.event_id, "note": "Go" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let event = signing.await.unwrap().expect("signed once approved");
    assert_eq!(event.id.to_hex(), request.event_id);
    assert_eq!(event.pubkey, app.bunker.public_key());
    assert!(event.verify().is_ok());
    assert_eq!(app.db.signature_count().unwrap(), 2);

    // Decided once.
    let again = app.signed(Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &admin).json(&json!({ "event_id": request.event_id })).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_standard_nip46_client_is_told_when_its_request_is_rejected() {
    let (_relay, app) = spawn_relay_app().await;
    let admin = app.admin.public_key().to_hex();
    let (client, _, bunker) = gated_client(&app).await;

    let signing = tokio::spawn(async move { client.sign_event(EventBuilder::text_note("off brand").build(bunker)).await });
    let request = waiting(&app).await;
    // Approving another event than the one shown is refused, and it still waits.
    let stale = app
        .signed(Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &admin)
        .json(&json!({ "event_id": EventId::all_zeros().to_hex() }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);

    let res = app
        .signed(Method::POST, &format!("/api/bunker/approvals/{}/reject", request.id), &admin)
        .json(&json!({ "note": "Not today" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let error = signing.await.unwrap().expect_err("rejected").to_string();
    assert!(error.contains("Rejected by an administrator: Not today"), "{}", error);
    assert_eq!(app.db.signature_count().unwrap(), 0);
}

#[tokio::test]
async fn a_standard_nip46_client_is_told_when_its_request_times_out() {
    let (_relay, app) = spawn_relay_app().await;
    app.state.nip46_handler.configure_approvals(chrono::Duration::seconds(1), None);
    let admin = app.admin.public_key().to_hex();
    let (client, _, bunker) = gated_client(&app).await;

    let signing = tokio::spawn(async move { client.sign_event(EventBuilder::text_note("nobody looked").build(bunker)).await });
    let request = waiting(&app).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    bunker::approvals_api::sweep(&app.state).await;

    let error = signing.await.unwrap().expect_err("timed out").to_string();
    assert!(error.contains("Approval timed out after 1 second: nothing was signed"), "{}", error);
    assert_eq!(app.db.signature_count().unwrap(), 0);
    let late = app.signed(Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &admin).json(&json!({ "event_id": request.event_id })).send().await.unwrap();
    assert_eq!(late.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_signer_sees_only_their_own_requests_waiting_and_cannot_decide_them() {
    let (_relay, app) = spawn_relay_app().await;
    let signer = app.register(Role::Signer);
    let admin = app.admin.public_key().to_hex();
    let token: IssueTokenResponse = issue(&app, &admin, json!({ "for_pubkey": signer.public_key().to_hex(), "label": "Bot", "kinds": [1], "approval_kinds": [1] }))
        .await
        .json()
        .await
        .unwrap();
    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(20), None).unwrap();
    let bunker = client.get_public_key().await.unwrap();
    let _signing = tokio::spawn(async move { client.sign_event(EventBuilder::text_note("mine").build(bunker)).await });
    let request = waiting(&app).await;

    let own: Vec<bunker::approvals_api::ApprovalResponse> =
        app.signed(Method::GET, "/api/bunker/approvals", &signer.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    assert_eq!(own.len(), 1);
    let other = app.register(Role::Signer);
    let none: Vec<bunker::approvals_api::ApprovalResponse> =
        app.signed(Method::GET, "/api/bunker/approvals", &other.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    assert!(none.is_empty());
    let refused = app
        .signed(Method::POST, &format!("/api/bunker/approvals/{}/approve", request.id), &signer.public_key().to_hex())
        .json(&json!({ "event_id": request.event_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let listed = connections(&app, &admin).await;
    assert_eq!(listed[0].approval_kinds, vec![1]);
}
