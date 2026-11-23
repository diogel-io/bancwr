// A member's own key held by the bunker (#138, spike for model A): evidence for the custody ADR.
// The key is added at the command line with `--owner`, as `bunker import-key --add --owner` does;
// its owner signs in to the console with a different key and issues their own tokens for it, and
// a standard NIP-46 client signs as it. Nobody else can use it: an administrator cannot issue a
// token on it, grant it or sign as it, and another member cannot issue on it. The key never signs
// in (rule 10, T2), and a bunker whose only other identity is a member's own key behaves as a
// single-key bunker for everyone else.
#[path = "../common/mod.rs"]
mod common;
#[path = "../support/ports.rs"]
mod ports;

use bunker::connections_api::IssueTokenResponse;
use bunker::identities_api::IdentityResponse;
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

/// `POST /api/bunker/connections/own-tokens` as `caller`.
async fn issue_own(app: &common::TestApp, caller: &Keys, body: Value) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/own-tokens", &caller.public_key().to_hex()).json(&body).send().await.unwrap()
}

/// `POST /api/bunker/connections/tokens` as the administrator.
async fn issue_as_admin(app: &common::TestApp, body: Value) -> reqwest::Response {
    app.signed(Method::POST, "/api/bunker/connections/tokens", &app.admin.public_key().to_hex()).json(&body).send().await.unwrap()
}

async fn error_of(res: reqwest::Response) -> (StatusCode, String) {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    (status, body["error"].as_str().unwrap_or_default().to_string())
}

#[tokio::test]
async fn the_owner_issues_a_token_for_their_own_key_and_an_app_signs_as_it_over_the_relay() {
    let relay = ports::run_mock_relay().await;
    let personal = Keys::generate();
    let console = Keys::generate();
    let app = common::spawn_with_owned_identities(true, Some(vec![relay.url()]), &[], &[("Gary", personal.clone(), console.clone())]).await;
    start_relay_client(&app, &relay.url()).await;

    let res = issue_own(&app, &console, json!({ "label": "Primal on my phone", "kinds": [1], "identity": personal.public_key().to_bech32().unwrap() })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert!(token.uri.starts_with(&format!("bunker://{}?", personal.public_key().to_hex())));
    assert_eq!(token.identity_pubkey, personal.public_key().to_hex());
    assert_eq!(token.for_pubkey, console.public_key().to_hex(), "always for the caller");

    let client = NostrConnect::new(NostrConnectURI::parse(&token.uri).unwrap(), Keys::generate(), Duration::from_secs(10), None).unwrap();
    assert_eq!(client.get_public_key().await.unwrap(), personal.public_key());
    let note = client.sign_event(EventBuilder::text_note("from my own key").build(personal.public_key())).await.unwrap();
    assert_eq!(note.pubkey, personal.public_key());
    assert!(note.verify().is_ok());
    // Only the kinds granted.
    assert!(client.sign_event(EventBuilder::new(Kind::Reaction, "+").build(personal.public_key())).await.is_err());

    // The log names the member's key as the identity, and the owner as the member.
    let logs: Vec<Value> = app.signed(Method::GET, "/api/bunker/logs", &app.admin.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    let logged = logs.iter().find(|l| l["event_id"] == note.id.to_hex()).expect("logged");
    assert_eq!(logged["identity_pubkey"], personal.public_key().to_hex());
    assert_eq!(logged["member_pubkey"], console.public_key().to_hex());
}

#[tokio::test]
async fn nobody_but_the_owner_can_issue_on_grant_or_sign_as_a_members_own_key() {
    let personal = Keys::generate();
    let console = Keys::generate();
    let app = common::spawn_with_owned_identities(true, Some(vec!["wss://relay.example".into()]), &[], &[("Gary", personal.clone(), console.clone())]).await;
    let identity = personal.public_key().to_hex();

    // An administrator, for the owner or for themselves, on the administrators' route.
    for for_pubkey in [console.public_key().to_hex(), app.admin.public_key().to_hex()] {
        let res = issue_as_admin(&app, json!({ "for_pubkey": for_pubkey, "label": "not theirs", "kinds": [1], "identity": identity })).await;
        assert_eq!(error_of(res).await, (StatusCode::FORBIDDEN, "owned_identity".to_string()));
    }
    // Nor on the owners' route, which is only ever for the caller's own key.
    let res = issue_own(&app, &app.admin, json!({ "label": "not theirs", "kinds": [1], "identity": identity })).await;
    assert_eq!(error_of(res).await, (StatusCode::FORBIDDEN, "not_your_identity".to_string()));
    // Another signer, too.
    let other = app.register(Role::Signer);
    let res = issue_own(&app, &other, json!({ "label": "not theirs", "kinds": [1], "identity": identity })).await;
    assert_eq!(error_of(res).await, (StatusCode::FORBIDDEN, "not_your_identity".to_string()));
    // The owners' route is never for the bunker's own identities.
    let res = issue_own(&app, &console, json!({ "label": "brand", "kinds": [1] })).await;
    assert_eq!(error_of(res).await, (StatusCode::FORBIDDEN, "not_your_identity".to_string()));
    // A viewer cannot call it at all.
    let viewer = app.register(Role::Viewer);
    assert_eq!(issue_own(&app, &viewer, json!({ "label": "x", "kinds": [1], "identity": identity })).await.status(), StatusCode::FORBIDDEN);

    // It is never granted to anyone, and a grant would change nothing.
    let path = format!("/api/bunker/identities/{}/members/{}", identity, other.public_key().to_hex());
    let res = app.signed(Method::PUT, &path, &app.admin.public_key().to_hex()).send().await.unwrap();
    assert_eq!(error_of(res).await, (StatusCode::BAD_REQUEST, "owned_identity".to_string()));
    assert!(!app.db.may_use_identity(&app.admin.public_key().to_hex(), Some(&identity)).unwrap(), "administrators have no access");
    assert!(!app.db.may_use_identity(&other.public_key().to_hex(), Some(&identity)).unwrap());
    assert!(app.db.may_use_identity(&console.public_key().to_hex(), Some(&identity)).unwrap());

    // Administrators see that it exists and whose it is, never a grant; other signers do not see it.
    let listed: Vec<IdentityResponse> = app.signed(Method::GET, "/api/bunker/identities", &app.admin.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    let owned = listed.iter().find(|i| i.pubkey == identity).expect("listed for administrators");
    assert_eq!(owned.owner.as_deref(), Some(console.public_key().to_hex().as_str()));
    let seen: Vec<IdentityResponse> = app.signed(Method::GET, "/api/bunker/identities", &other.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    assert!(seen.iter().all(|i| i.pubkey != identity));
    let mine: Vec<IdentityResponse> = app.signed(Method::GET, "/api/bunker/identities", &console.public_key().to_hex()).send().await.unwrap().json().await.unwrap();
    assert!(mine.iter().any(|i| i.pubkey == identity));
}

#[tokio::test]
async fn a_members_own_key_leaves_a_single_key_bunker_working_as_before() {
    let personal = Keys::generate();
    let console = Keys::generate();
    let app = common::spawn_with_owned_identities(true, Some(vec!["wss://relay.example".into()]), &[], &[("Gary", personal.clone(), console.clone())]).await;

    // Not "several identities": a signer needs no grant for the primary, and an administrator
    // issues for them as on a single-key bunker.
    assert!(!app.db.has_several_identities().unwrap());
    let signer = app.register(Role::Signer);
    assert!(app.db.may_use_identity(&signer.public_key().to_hex(), None).unwrap());
    let res = issue_as_admin(&app, json!({ "for_pubkey": signer.public_key().to_hex(), "label": "brand app", "kinds": [1] })).await;
    assert_eq!(res.status(), StatusCode::OK);
    let token: IssueTokenResponse = res.json().await.unwrap();
    assert_eq!(token.identity_pubkey, app.bunker.public_key().to_hex());
}

#[tokio::test]
async fn a_members_own_key_never_signs_in_or_becomes_a_member() {
    let personal = Keys::generate();
    let console = Keys::generate();
    let app = common::spawn_with_owned_identities(true, None, &[], &[("Gary", personal.clone(), console.clone())]).await;

    // The status lists it with the bunker's keys, which sign-in refuses (403 bunker_key).
    let status: Value = app.get("/api/bunker/status").send().await.unwrap().json().await.unwrap();
    assert_eq!(status["identities"], json!([app.bunker.public_key().to_hex(), personal.public_key().to_hex()]));
    // And it can never be registered as a member.
    let res = app
        .signed(Method::POST, "/api/bunker/team", &app.admin.public_key().to_hex())
        .json(&json!({ "name": "Gary's own key", "pubkey": personal.public_key().to_bech32().unwrap(), "role": "signer" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_owner_made_a_viewer_can_no_longer_issue_for_their_own_key() {
    let personal = Keys::generate();
    let console = Keys::generate();
    let app = common::spawn_with_owned_identities(true, Some(vec!["wss://relay.example".into()]), &[], &[("Gary", personal.clone(), console.clone())]).await;
    let identity = personal.public_key().to_hex();
    assert_eq!(issue_own(&app, &console, json!({ "label": "before", "kinds": [1], "identity": identity })).await.status(), StatusCode::OK);

    // Administrators keep role management (T4): demoting the owner revokes their tokens and stops
    // them issuing more, but gives the administrator nothing.
    let member = app.db.find_member_by_pubkey(&console.public_key().to_hex()).unwrap().unwrap();
    let res = app
        .signed(Method::PATCH, &format!("/api/bunker/team/{}", member.id), &app.admin.public_key().to_hex())
        .json(&json!({ "role": "viewer" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(issue_own(&app, &console, json!({ "label": "after", "kinds": [1], "identity": identity })).await.status(), StatusCode::FORBIDDEN);
    assert!(app.db.list_nip46_tokens().unwrap().iter().all(|t| t.revoked_at.is_some()), "their tokens are revoked");
    assert!(!app.db.may_use_identity(&app.admin.public_key().to_hex(), Some(&identity)).unwrap());
}

#[test]
fn an_owner_must_be_a_member_who_can_sign() {
    use bunker::db::{AddIdentityRefusal, Database};
    let db = Database::new(":memory:").unwrap();
    let viewer = Keys::generate();
    db.add_team_member("Vera", &viewer.public_key().to_hex(), Role::Viewer).unwrap();
    let now = chrono::Utc::now();
    let key = Keys::generate().public_key().to_hex();
    assert_eq!(db.add_owned_identity(&key, "Vera's key", "line", &viewer.public_key().to_hex(), now).unwrap(), Err(AddIdentityRefusal::OwnerCannotSign));
    assert_eq!(db.add_owned_identity(&key, "Nobody's key", "line", &Keys::generate().public_key().to_hex(), now).unwrap(), Err(AddIdentityRefusal::OwnerCannotSign));
    // The owner's own console key can never be the key held.
    assert_eq!(db.add_owned_identity(&viewer.public_key().to_hex(), "x", "line", &viewer.public_key().to_hex(), now).unwrap(), Err(AddIdentityRefusal::IsAMember));
}
