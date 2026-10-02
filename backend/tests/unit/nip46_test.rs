// NIP-46 behind connection tokens (#53): every refusal path, the grant intersection, logout,
// persistence across a reopen, and member removal ending a connection.
use bunker::db::Database;
use bunker::nip46::{hash_secret, sign_permission, Nip46Handler, Nip46Request, Nip46Response, NOT_CONNECTED};
use bunker::registry::Role;
use bunker::signer::Signer;
use chrono::{Duration, Utc};
use nostr::prelude::*;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

struct Setup {
    _dir: TempDir,
    path: String,
    db: Database,
    bunker: Keys,
    member: Keys,
    handler: Nip46Handler,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db").to_str().unwrap().to_string();
    let db = Database::new(&path).unwrap();
    let bunker = Keys::generate();
    let member = Keys::generate();
    db.add_team_member("Member", &member.public_key().to_hex(), Role::User).unwrap();
    let handler = handler_for(&db, &bunker);
    Setup { _dir: dir, path, db, bunker, member, handler }
}

fn handler_for(db: &Database, bunker: &Keys) -> Nip46Handler {
    Nip46Handler::new(Arc::new(RwLock::new(Signer::new(bunker.secret_key().clone()))), db.clone())
}

/// Issues a token for the setup's member allowing `kinds`, valid for `hours`; returns its secret.
fn issue(s: &Setup, kinds: &[u16], hours: i64) -> String {
    let secret = format!("secret-{}", uuid::Uuid::new_v4());
    let perms: Vec<String> = kinds.iter().map(|k| sign_permission(*k)).collect();
    let now = Utc::now();
    s.db.create_nip46_token(&hash_secret(&secret), &s.member.public_key().to_hex(), "admin", "test", &perms, now, now + Duration::hours(hours)).unwrap();
    secret
}

fn request(method: &str, params: Vec<String>) -> Nip46Request {
    Nip46Request { id: format!("{}-id", method), method: method.to_string(), params }
}

async fn connect(s: &Setup, client: &Keys, params: Vec<String>) -> Nip46Response {
    s.handler.handle_request(request("connect", params), client.public_key()).await
}

fn connect_params(s: &Setup, secret: &str) -> Vec<String> {
    vec![s.bunker.public_key().to_hex(), secret.to_string()]
}

async fn sign(s: &Setup, client: &Keys, kind: Kind) -> Nip46Response {
    let unsigned = EventBuilder::new(kind, "hello").build(client.public_key());
    s.handler.handle_request(request("sign_event", vec![serde_json::to_string(&unsigned).unwrap()]), client.public_key()).await
}

#[tokio::test]
async fn connects_with_a_token_and_signs_a_granted_kind_as_the_bunker() {
    let s = setup();
    let secret = issue(&s, &[1, 7], 24);
    let client = Keys::generate();

    let connected = connect(&s, &client, connect_params(&s, &secret)).await;
    assert_eq!(connected.result.as_deref(), Some("ack"), "{:?}", connected.error);

    let signed = sign(&s, &client, Kind::TextNote).await;
    let event: Event = serde_json::from_str(signed.result.as_deref().expect("signed")).unwrap();
    assert_eq!(event.pubkey, s.bunker.public_key());
    assert!(event.verify().is_ok());

    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.for_pubkey, s.member.public_key().to_hex());
    assert!(connection.last_used_at.is_some());
}

#[tokio::test]
async fn refuses_a_kind_outside_the_grant() {
    let s = setup();
    let secret = issue(&s, &[1], 24);
    let client = Keys::generate();
    connect(&s, &client, connect_params(&s, &secret)).await;

    for kind in [Kind::Metadata, Kind::ContactList, Kind::EventDeletion, Kind::from(27235)] {
        let refused = sign(&s, &client, kind).await;
        assert!(refused.result.is_none(), "{:?}", kind);
        assert!(refused.error.unwrap().starts_with("Forbidden"));
    }
}

#[tokio::test]
async fn refuses_connect_without_a_secret_with_a_wrong_one_or_to_another_signer() {
    let s = setup();
    issue(&s, &[1], 24);
    let client = Keys::generate();

    for params in [
        vec![s.bunker.public_key().to_hex()],
        vec![s.bunker.public_key().to_hex(), String::new()],
        vec![s.bunker.public_key().to_hex(), "wrong".to_string()],
        vec![Keys::generate().public_key().to_hex(), "wrong".to_string()],
        vec![],
    ] {
        let refused = connect(&s, &client, params.clone()).await;
        assert!(refused.result.is_none(), "{:?}", params);
    }
    // And nothing it asks for afterwards is answered.
    let ping = s.handler.handle_request(request("ping", vec![]), client.public_key()).await;
    assert_eq!(ping.error.as_deref(), Some(NOT_CONNECTED));
    assert_eq!(sign(&s, &client, Kind::TextNote).await.error.as_deref(), Some(NOT_CONNECTED));
}

#[tokio::test]
async fn a_token_connects_once_only() {
    let s = setup();
    let secret = issue(&s, &[1], 24);
    let first = Keys::generate();
    let second = Keys::generate();
    assert_eq!(connect(&s, &first, connect_params(&s, &secret)).await.result.as_deref(), Some("ack"));
    let reused = connect(&s, &second, connect_params(&s, &secret)).await;
    assert_eq!(reused.error.as_deref(), Some("This connection token has already been used"));
}

#[tokio::test]
async fn refuses_an_expired_or_revoked_token() {
    let s = setup();
    let expired = issue(&s, &[1], -1);
    assert_eq!(connect(&s, &Keys::generate(), connect_params(&s, &expired)).await.error.as_deref(), Some("This connection token has expired"));

    let revoked = issue(&s, &[1], 24);
    let token = s.db.list_nip46_tokens().unwrap().into_iter().find(|t| t.used_at.is_none() && t.expires_at > Utc::now()).unwrap();
    assert!(s.db.revoke_nip46_token(&token.id, Utc::now()).unwrap());
    assert_eq!(connect(&s, &Keys::generate(), connect_params(&s, &revoked)).await.error.as_deref(), Some("This connection token has been revoked"));
}

#[tokio::test]
async fn grants_only_what_both_the_token_and_the_request_allow() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1, 7], 24);
    let mut params = connect_params(&s, &secret);
    params.push("sign_event:1,sign_event:4,nip44_encrypt".to_string());
    assert_eq!(connect(&s, &client, params).await.result.as_deref(), Some("ack"));
    assert_eq!(s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap().perms, vec!["sign_event:1"]);
    assert!(sign(&s, &client, Kind::Reaction).await.error.is_some());

    // Asking only for what the token does not allow connects nothing, and leaves the token unused.
    let other = issue(&s, &[1], 24);
    let mut params = connect_params(&s, &other);
    params.push("sign_event:4".to_string());
    let refused = connect(&s, &Keys::generate(), params).await;
    assert_eq!(refused.error.as_deref(), Some("None of the requested permissions are allowed by this connection token"));
    let mut retry = connect_params(&s, &other);
    retry.push(String::new());
    assert_eq!(connect(&s, &Keys::generate(), retry).await.result.as_deref(), Some("ack"));
}

#[tokio::test]
async fn keeps_client_metadata_as_unverified_display_hints() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    let mut params = connect_params(&s, &secret);
    params.push(String::new());
    params.push(r#"{"name":"  Damus  ","url":"https://damus.io","image":"javascript:alert(1)"}"#.to_string());
    connect(&s, &client, params).await;
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.client_name.as_deref(), Some("Damus"));
    assert_eq!(connection.client_url.as_deref(), Some("https://damus.io"));
    assert_eq!(connection.client_image, None);
}

#[tokio::test]
async fn answers_get_public_key_with_hex_and_refuses_encryption_methods() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;
    let key = s.handler.handle_request(request("get_public_key", vec![]), client.public_key()).await;
    assert_eq!(key.result, Some(s.bunker.public_key().to_hex()));
    let pong = s.handler.handle_request(request("ping", vec![]), client.public_key()).await;
    assert_eq!(pong.result.as_deref(), Some("pong"));
    let nip44 = s.handler.handle_request(request("nip44_encrypt", vec!["x".into(), "y".into()]), client.public_key()).await;
    assert_eq!(nip44.error.as_deref(), Some("Not supported: nip44_encrypt"));
}

#[tokio::test]
async fn logout_ends_the_connection() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;
    let ack = s.handler.handle_request(request("logout", vec![]), client.public_key()).await;
    assert_eq!(ack.result.as_deref(), Some("ack"));
    assert_eq!(sign(&s, &client, Kind::TextNote).await.error.as_deref(), Some(NOT_CONNECTED));
    let all = s.db.list_nip46_connections(None).unwrap();
    assert_eq!(all[0].revoked_reason.as_deref(), Some("logout"));
}

#[tokio::test]
async fn a_connection_survives_a_restart() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;

    // A new database handle and handler on the same file, as after a restart.
    let reopened = Database::new(&s.path).unwrap();
    let after = Setup { _dir: tempfile::tempdir().unwrap(), path: s.path.clone(), handler: handler_for(&reopened, &s.bunker), db: reopened, bunker: s.bunker.clone(), member: s.member.clone() };
    assert!(sign(&after, &client, Kind::TextNote).await.result.is_some());
    assert_eq!(after.handler.connection_count().await, 1);
}

#[tokio::test]
async fn removing_the_member_ends_their_connections_and_tokens() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    let unused = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;
    // Another administrator, so the member can be removed.
    s.db.add_team_member("Admin", &Keys::generate().public_key().to_hex(), Role::Administrator).unwrap();

    let member = s.db.find_member_by_pubkey(&s.member.public_key().to_hex()).unwrap().unwrap();
    s.db.remove_team_member(member.id).unwrap();

    assert_eq!(sign(&s, &client, Kind::TextNote).await.error.as_deref(), Some(NOT_CONNECTED));
    assert_eq!(s.db.list_nip46_connections(None).unwrap()[0].revoked_reason.as_deref(), Some("member_removed"));
    let refused = connect(&s, &Keys::generate(), connect_params(&s, &unused)).await;
    assert_eq!(refused.error.as_deref(), Some("This connection token has been revoked"));
}

#[tokio::test]
async fn a_client_that_connects_again_replaces_its_connection() {
    let s = setup();
    let client = Keys::generate();
    let first = issue(&s, &[1], 24);
    let second = issue(&s, &[7], 24);
    connect(&s, &client, connect_params(&s, &first)).await;
    connect(&s, &client, connect_params(&s, &second)).await;
    assert_eq!(s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap().perms, vec!["sign_event:7"]);
    let all = s.db.list_nip46_connections(None).unwrap();
    assert_eq!(all.iter().filter(|c| c.revoked_reason.as_deref() == Some("replaced")).count(), 1);
}

#[tokio::test]
async fn signs_a_template_without_a_pubkey_as_nip46_defines_it() {
    // nostr-tools' BunkerSigner sends {kind, content, tags, created_at} only (#31).
    let s = setup();
    let secret = issue(&s, &[1], 24);
    let client = Keys::generate();
    connect(&s, &client, connect_params(&s, &secret)).await;

    let template = r#"{"kind":1,"content":"hello","tags":[["t","nostr"]],"created_at":1700000000}"#;
    let signed = s.handler.handle_request(request("sign_event", vec![template.to_string()]), client.public_key()).await;
    let event: Event = serde_json::from_str(signed.result.as_deref().expect("signed")).unwrap();
    assert_eq!(event.pubkey, s.bunker.public_key());
    assert_eq!(event.content, "hello");
    assert_eq!(event.created_at.as_u64(), 1_700_000_000);
    assert!(event.verify().is_ok());

    // A pubkey or id the client supplies does not change who signs.
    let other = Keys::generate().public_key().to_hex();
    let template = format!(r#"{{"kind":1,"content":"x","tags":[],"created_at":1700000000,"pubkey":"{}","id":"{}"}}"#, other, "0".repeat(64));
    let signed = s.handler.handle_request(request("sign_event", vec![template]), client.public_key()).await;
    let event: Event = serde_json::from_str(signed.result.as_deref().expect("signed")).unwrap();
    assert_eq!(event.pubkey, s.bunker.public_key());
    assert!(event.verify().is_ok());

    let refused = s.handler.handle_request(request("sign_event", vec!["[1]".to_string()]), client.public_key()).await;
    assert!(refused.error.unwrap().starts_with("Invalid event JSON"));
}
