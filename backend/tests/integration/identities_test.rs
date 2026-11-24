// Several identities in one bunker (#103), through the API and a standard NIP-46 client: listing
// and renaming identities, granting members, issuing a connection for a second identity that signs
// as it, a member granted only one identity unable to sign as another, an app connected to one
// identity refused by another, removing a grant revoking what it allowed, identity keys refused as
// members, and logs and exports naming the identity.
#[path = "../common/mod.rs"]
mod common;
#[path = "../support/ports.rs"]
mod ports;

use bunker::approvals::Scheme;
use bunker::connections_api::{ConnectionResponse, IssueTokenResponse};
use bunker::identities_api::IdentityResponse;
use bunker::nip46::{Nip46Outcome, Nip46Request, NOT_CONNECTED, TOKEN_USED_UP};
use bunker::registry::Role;
use bunker::relay::RelayClient;
use nostr_connect::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

async fn start_relay_client(app: &common::TestApp, relay: &str) {
    let client = RelayClient::new(vec![relay.to_string()], app.state.clone()).await.unwrap();
    tokio::spawn(async move {
        let _ = client.run().await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
}

async fn issue(app: &common::TestApp, body: Value) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/tokens", &app.admin.public_key().to_hex()).json(&body).send().await.unwrap()
}

async fn identities(app: &common::TestApp, as_identity: &str) -> Vec<IdentityResponse> {
    let res = app.signed(Method::GET, "/api/bunker/identities", as_identity).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

async fn grant(app: &common::TestApp, identity: &Keys, member: &Keys) -> StatusCode {
    let path = format!("/api/bunker/identities/{}/members/{}", identity.public_key().to_hex(), member.public_key().to_hex());
    app.signed(Method::PUT, &path, &app.admin.public_key().to_hex()).send().await.unwrap().status()
}

#[tokio::test]
async fn an_administrator_sees_every_identity_and_a_signer_only_those_granted() {
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example".into()]), &[("Brand B", brand.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    let signer = app.register(Role::Signer);

    let all = identities(&app, &admin).await;
    assert_eq!(all.iter().map(|i| (i.label.as_str(), i.is_primary, i.available)).collect::<Vec<_>>(), vec![("Primary", true, true), ("Brand B", false, true)]);
    assert_eq!(all[0].pubkey, app.bunker.public_key().to_hex());
    assert_eq!(all[1].npub.as_deref(), Some(brand.public_key().to_bech32().unwrap().as_str()));
    // The administrator held the primary when the second identity arrived, so was granted it.
    assert_eq!(all[0].members.as_deref(), Some(&[admin.clone()][..]));
    assert_eq!(all[1].members.as_deref(), Some(&[][..]));

    // A signer registered since has no grant yet, so sees none, then only what is granted.
    assert!(identities(&app, &signer.public_key().to_hex()).await.is_empty());
    assert_eq!(grant(&app, &brand, &signer).await, StatusCode::OK);
    let seen = identities(&app, &signer.public_key().to_hex()).await;
    assert_eq!(seen.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["Brand B"]);
    assert!(seen[0].members.is_none(), "members are for administrators");

    // Renamed, and the label is checked.
    let path = format!("/api/bunker/identities/{}", brand.public_key().to_bech32().unwrap());
    assert_eq!(app.signed(Method::PATCH, &path, &admin).json(&json!({ "label": "Brand Bee" })).send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(identities(&app, &admin).await[1].label, "Brand Bee");
    assert_eq!(app.signed(Method::PATCH, &path, &admin).json(&json!({ "label": " " })).send().await.unwrap().status(), StatusCode::BAD_REQUEST);
    let unknown = format!("/api/bunker/identities/{}", Keys::generate().public_key().to_hex());
    assert_eq!(app.signed(Method::PATCH, &unknown, &admin).json(&json!({ "label": "x" })).send().await.unwrap().status(), StatusCode::NOT_FOUND);

    // A viewer is never granted one.
    let viewer = app.register(Role::Viewer);
    assert_eq!(grant(&app, &brand, &viewer).await, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_connection_issued_for_a_second_identity_signs_as_it_over_the_relay() {
    let relay = ports::run_mock_relay().await;
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec![relay.url()]), &[("Brand B", brand.clone())]).await;
    start_relay_client(&app, &relay.url()).await;
    let admin = app.admin.public_key().to_hex();

    let res = issue(&app, json!({ "label": "Brand B app", "kinds": [1], "identity": brand.public_key().to_bech32().unwrap() })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert!(token.uri.starts_with(&format!("bunker://{}?", brand.public_key().to_hex())));
    assert_eq!(token.identity_pubkey, brand.public_key().to_hex());

    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    assert_eq!(client.get_public_key().await.unwrap(), brand.public_key());
    let note = client.sign_event(EventBuilder::text_note("from Brand B").build(brand.public_key())).await.unwrap();
    assert_eq!(note.pubkey, brand.public_key());
    assert!(note.verify().is_ok());

    // The primary still answers its own apps, on the same relay.
    let primary: IssueTokenResponse = issue(&app, json!({ "label": "Primary app", "kinds": [1] })).await.json().await.unwrap();
    assert_eq!(primary.identity_pubkey, app.bunker.public_key().to_hex());
    let other = NostrConnect::new(NostrConnectURI::parse(&primary.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    assert_eq!(other.sign_event(EventBuilder::text_note("from the primary").build(app.bunker.public_key())).await.unwrap().pubkey, app.bunker.public_key());

    // The log and the connections name the identity.
    let logs: Vec<Value> = app.signed(Method::GET, "/api/bunker/logs", &admin).send().await.unwrap().json().await.unwrap();
    let brand_log = logs.iter().find(|l| l["event_id"] == note.id.to_hex()).expect("logged");
    assert_eq!(brand_log["identity_pubkey"], brand.public_key().to_hex());
    let connections: Vec<ConnectionResponse> = app.signed(Method::GET, "/api/bunker/connections", &admin).send().await.unwrap().json().await.unwrap();
    let mut named: Vec<String> = connections.iter().filter_map(|c| c.identity_pubkey.clone()).collect();
    named.sort();
    let mut expected = vec![brand.public_key().to_hex(), app.bunker.public_key().to_hex()];
    expected.sort();
    assert_eq!(named, expected);

    // The export's last column names it too.
    let day = chrono::Utc::now().format("%Y-%m-%d");
    let csv = app.signed(Method::GET, &format!("/api/bunker/logs/export?format=csv&from={day}&to={day}"), &admin).send().await.unwrap().text().await.unwrap();
    let line = csv.lines().find(|l| l.contains(&note.id.to_hex())).expect("exported");
    assert!(line.ends_with(&format!(",{}", brand.public_key().to_hex())), "{line}");
}

#[tokio::test]
async fn a_signer_issuing_their_own_token_signs_only_as_an_identity_they_are_granted() {
    let brand = Keys::generate();
    let other_brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example".into()]), &[("Brand B", brand.clone()), ("Brand C", other_brand.clone())]).await;
    let signer = app.register(Role::Signer);
    assert_eq!(grant(&app, &brand, &signer).await, StatusCode::OK);
    let own = |identity: &Keys| json!({ "label": "Mine", "kinds": [1], "identity": identity.public_key().to_hex() });
    let as_signer = |body: Value| app.signed(Method::POST, "/api/bunker/connections/tokens", &signer.public_key().to_hex()).json(&body).send();

    let res = as_signer(own(&brand)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let issued: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(issued.identity_pubkey, brand.public_key().to_hex());
    assert!(issued.uri.starts_with(&format!("bunker://{}?", brand.public_key().to_hex())));

    // Not granted: Brand C, nor the primary once there are several identities.
    let refused = as_signer(own(&other_brand)).await.unwrap();
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "no_identity_access");
    let refused = as_signer(json!({ "label": "Mine", "kinds": [1] })).await.unwrap();
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "no_identity_access");
}

#[tokio::test]
async fn a_member_granted_only_one_identity_cannot_sign_as_another() {
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example".into()]), &[("Brand B", brand.clone())]).await;
    let admin = app.admin.public_key().to_hex();

    // Added through the API with only Brand B.
    let signer = Keys::generate();
    let res = app
        .signed(Method::POST, "/api/bunker/team", &admin)
        .json(&json!({ "name": "Sam", "pubkey": signer.public_key().to_hex(), "role": "signer", "identities": [brand.public_key().to_hex()] }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let for_signer = |identity: Option<String>| {
        let mut body = json!({ "for_pubkey": signer.public_key().to_hex(), "label": "x", "kinds": [1] });
        if let Some(identity) = identity {
            body["identity"] = json!(identity);
        }
        body
    };
    let refused = issue(&app, for_signer(None)).await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(refused.json::<Value>().await.unwrap()["error"], "no_identity_access");
    assert_eq!(issue(&app, for_signer(Some(brand.public_key().to_hex()))).await.status(), StatusCode::OK);
    let unknown = issue(&app, for_signer(Some(Keys::generate().public_key().to_hex()))).await;
    assert_eq!(unknown.json::<Value>().await.unwrap()["error"], "invalid_identity");

    // Added without naming any, a member gets the primary only.
    let other = Keys::generate();
    app.signed(Method::POST, "/api/bunker/team", &admin)
        .json(&json!({ "name": "Pat", "pubkey": other.public_key().to_hex(), "role": "signer" }))
        .send()
        .await
        .unwrap();
    assert!(app.db.may_use_identity(&other.public_key().to_hex(), None).unwrap());
    assert!(!app.db.may_use_identity(&other.public_key().to_hex(), Some(&brand.public_key().to_hex())).unwrap());
}

/// Connects `client` for the administrator through the handler, with a token for `identity`.
async fn connect_through_handler(app: &common::TestApp, identity: Option<&Keys>, client: &Keys) {
    let mut body = json!({ "label": "x", "kinds": [1] });
    if let Some(identity) = identity {
        body["identity"] = json!(identity.public_key().to_hex());
    }
    let token: IssueTokenResponse = issue(app, body).await.json().await.unwrap();
    let secret = token.uri.split("secret=").nth(1).unwrap().to_string();
    let target = identity.map(|k| k.public_key()).unwrap_or(app.bunker.public_key());
    let request = Nip46Request { id: "c".into(), method: "connect".into(), params: vec![target.to_hex(), secret] };
    let outcome = app.state.nip46_handler.handle_for(request, client.public_key(), Scheme::Nip44, identity.map(|k| k.public_key().to_hex())).await;
    let Nip46Outcome::Reply(response) = outcome else { panic!("not held") };
    assert_eq!(response.result.as_deref(), Some("ack"), "{:?}", response.error);
}

#[tokio::test]
async fn an_app_connected_to_one_identity_is_not_connected_to_another() {
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example".into()]), &[("Brand B", brand.clone())]).await;
    let client = Keys::generate();
    connect_through_handler(&app, None, &client).await;

    let ask = |identity: Option<String>| {
        let request = Nip46Request { id: "k".into(), method: "get_public_key".into(), params: vec![] };
        app.state.nip46_handler.handle_for(request, client.public_key(), Scheme::Nip44, identity)
    };
    let Nip46Outcome::Reply(own) = ask(None).await else { panic!() };
    assert_eq!(own.result, Some(app.bunker.public_key().to_hex()));
    let Nip46Outcome::Reply(other) = ask(Some(brand.public_key().to_hex())).await else { panic!() };
    assert_eq!(other.error.as_deref(), Some(NOT_CONNECTED));

    // A token for Brand B does not connect to the primary.
    let token: IssueTokenResponse = issue(&app, json!({ "label": "x", "kinds": [1], "identity": brand.public_key().to_hex() })).await.json().await.unwrap();
    let secret = token.uri.split("secret=").nth(1).unwrap().to_string();
    let request = Nip46Request { id: "c".into(), method: "connect".into(), params: vec![app.bunker.public_key().to_hex(), secret.clone()] };
    let Nip46Outcome::Reply(refused) = app.state.nip46_handler.handle_for(request, Keys::generate().public_key(), Scheme::Nip44, None).await else { panic!() };
    assert_eq!(refused.error.as_deref(), Some(format!("This connection token is for another identity{}", TOKEN_USED_UP).as_str()));

    // That used the token up: addressed to Brand B afterwards, it is refused as used, and the
    // token list shows it used.
    let request = Nip46Request { id: "c".into(), method: "connect".into(), params: vec![brand.public_key().to_hex(), secret] };
    let Nip46Outcome::Reply(again) =
        app.state.nip46_handler.handle_for(request, Keys::generate().public_key(), Scheme::Nip44, Some(brand.public_key().to_hex())).await
    else {
        panic!()
    };
    assert_eq!(again.error.as_deref(), Some("This connection token has already been used"));
    let tokens: Vec<Value> =
        app.signed(Method::GET, "/api/bunker/connections/tokens", &app.admin.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    let listed = tokens.iter().find(|t| t["id"] == json!(token.id)).unwrap();
    assert!(listed["used_at"].is_string() && listed["revoked_at"].is_null(), "{}", listed);
}

#[tokio::test]
async fn removing_a_grant_revokes_the_members_connections_and_tokens_for_that_identity_only() {
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, Some(vec!["wss://relay.example".into()]), &[("Brand B", brand.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    let signer = app.register(Role::Signer);
    assert_eq!(grant(&app, &brand, &signer).await, StatusCode::OK);
    app.db.grant_identity(None, &signer.public_key().to_hex(), None, chrono::Utc::now()).unwrap();

    let for_signer = |identity: Option<&Keys>| {
        let mut body = json!({ "for_pubkey": signer.public_key().to_hex(), "label": "x", "kinds": [1] });
        if let Some(identity) = identity {
            body["identity"] = json!(identity.public_key().to_hex());
        }
        body
    };
    // A connection and an unused token for each identity.
    for identity in [Some(&brand), None] {
        let token: IssueTokenResponse = issue(&app, for_signer(identity)).await.json().await.unwrap();
        let secret = token.uri.split("secret=").nth(1).unwrap().to_string();
        let target = identity.map(|k| k.public_key()).unwrap_or(app.bunker.public_key());
        let request = Nip46Request { id: "c".into(), method: "connect".into(), params: vec![target.to_hex(), secret] };
        app.state.nip46_handler.handle_for(request, Keys::generate().public_key(), Scheme::Nip44, identity.map(|k| k.public_key().to_hex())).await;
        issue(&app, for_signer(identity)).await;
    }

    let path = format!("/api/bunker/identities/{}/members/{}", brand.public_key().to_hex(), signer.public_key().to_hex());
    let res = app.signed(Method::DELETE, &path, &admin).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.json::<Value>().await.unwrap(), json!({ "success": true, "connections_revoked": 1, "tokens_revoked": 1 }));
    let connections = app.db.list_nip46_connections(Some(&signer.public_key().to_hex())).unwrap();
    let brand_hex = brand.public_key().to_hex();
    let brand_connection = connections.iter().find(|c| c.identity_pubkey.as_deref() == Some(brand_hex.as_str())).unwrap();
    assert_eq!(brand_connection.revoked_reason.as_deref(), Some("access_removed"));
    assert!(connections.iter().any(|c| c.identity_pubkey.is_none() && c.revoked_at.is_none()), "the primary's connection is untouched");
    assert_eq!(app.signed(Method::DELETE, &path, &admin).send().await.unwrap().status(), StatusCode::NOT_FOUND);
    let removed = app.db.audit_events().unwrap().into_iter().find(|e| e.action == "identity_access_removed").unwrap();
    assert_eq!((removed.actor_pubkey.as_deref(), removed.identity_pubkey.as_deref()), (Some(admin.as_str()), Some(brand_hex.as_str())));
}

#[tokio::test]
async fn no_identity_key_can_be_a_member_and_the_status_lists_them_all() {
    let brand = Keys::generate();
    let app = common::spawn_with_identities(true, None, &[("Brand B", brand.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    for key in [brand.public_key(), app.bunker.public_key()] {
        let res = app
            .signed(Method::POST, "/api/bunker/team", &admin)
            .json(&json!({ "name": "Not a person", "pubkey": key.to_bech32().unwrap(), "role": "signer" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
    let status: Value = app.get("/api/bunker/status").send().await.unwrap().json().await.unwrap();
    assert_eq!(status["identities"], json!([app.bunker.public_key().to_hex(), brand.public_key().to_hex()]));
}
