// NIP-46 behind connection tokens (#53): every refusal path, the grant intersection, logout,
// persistence across a reopen, and member removal ending a connection. Plus the encryption
// methods (#97): their permissions, parameters, the refusal of NIP-46 app keys as a peer, and the
// audit event each decrypt leaves before it happens.
use bunker::db::Database;
use bunker::nip46::{hash_secret, sign_permission, ENCRYPTION_METHODS, EXPIRED, Nip46Handler, Nip46Request, Nip46Response, NOT_CONNECTED};
use bunker::registry::Role;
use bunker::signer::Signer;
use chrono::{Duration, SubsecRound, Utc};
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
    db.add_team_member("Member", &member.public_key().to_hex(), Role::Signer).unwrap();
    let handler = handler_for(&db, &bunker);
    Setup { _dir: dir, path, db, bunker, member, handler }
}

fn handler_for(db: &Database, bunker: &Keys) -> Nip46Handler {
    Nip46Handler::new(Arc::new(RwLock::new(Signer::new(bunker.secret_key().clone()))), db.clone())
}

/// Issues a token for the setup's member allowing `kinds`, valid for `hours`; returns its secret.
fn issue(s: &Setup, kinds: &[u16], hours: i64) -> String {
    let perms: Vec<String> = kinds.iter().map(|k| sign_permission(*k)).collect();
    issue_perms(s, &perms, hours)
}

/// Issues a token for the setup's member with exactly `perms`; returns its secret.
fn issue_perms(s: &Setup, perms: &[String], hours: i64) -> String {
    let secret = format!("secret-{}", uuid::Uuid::new_v4());
    let now = Utc::now();
    s.db.create_nip46_token(&hash_secret(&secret), &s.member.public_key().to_hex(), "admin", "test", perms, now, now + Duration::hours(hours)).unwrap();
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
async fn answers_get_public_key_with_hex_and_refuses_encryption_methods_not_granted() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;
    let key = s.handler.handle_request(request("get_public_key", vec![]), client.public_key()).await;
    assert_eq!(key.result, Some(s.bunker.public_key().to_hex()));
    let pong = s.handler.handle_request(request("ping", vec![]), client.public_key()).await;
    assert_eq!(pong.result.as_deref(), Some("pong"));
    let nip44 = s.handler.handle_request(request("nip44_encrypt", vec!["x".into(), "y".into()]), client.public_key()).await;
    assert_eq!(nip44.error.as_deref(), Some("Forbidden: this connection may not call nip44_encrypt"));
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

/// A connected client granted `methods` (and kind 1), and its keys.
async fn connected_with(s: &Setup, methods: &[&str]) -> Keys {
    let mut perms = vec![sign_permission(1)];
    perms.extend(methods.iter().map(|m| m.to_string()));
    let secret = issue_perms(s, &perms, 24);
    let client = Keys::generate();
    let connected = connect(s, &client, connect_params(s, &secret)).await;
    assert_eq!(connected.result.as_deref(), Some("ack"), "{:?}", connected.error);
    client
}

async fn call(s: &Setup, client: &Keys, method: &str, peer: &PublicKey, payload: &str) -> Nip46Response {
    s.handler.handle_request(request(method, vec![peer.to_hex(), payload.to_string()]), client.public_key()).await
}

fn decrypt_events(s: &Setup) -> Vec<bunker::audit::ExportRow> {
    s.db.audit_events().unwrap().into_iter().filter(|e| e.action.ends_with("_decrypt")).collect()
}

#[tokio::test]
async fn encrypts_and_decrypts_nip44_with_the_bunkers_key() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt"]).await;
    let friend = Keys::generate();

    // What the bunker encrypts to a friend, the friend can read, and the other way round.
    let sent = call(&s, &client, "nip44_encrypt", &friend.public_key(), "hello friend").await;
    let ciphertext = sent.result.expect("encrypted");
    assert_eq!(nip44::decrypt(friend.secret_key(), &s.bunker.public_key(), &ciphertext).unwrap(), "hello friend");
    let incoming = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "hello brand", nip44::Version::V2).unwrap();
    let read = call(&s, &client, "nip44_decrypt", &friend.public_key(), &incoming).await;
    assert_eq!(read.result.as_deref(), Some("hello brand"), "{:?}", read.error);

    // To itself, as NIP-51 private lists do.
    let own = call(&s, &client, "nip44_encrypt", &s.bunker.public_key(), "private list").await.result.unwrap();
    let back = call(&s, &client, "nip44_decrypt", &s.bunker.public_key(), &own).await;
    assert_eq!(back.result.as_deref(), Some("private list"));
}

#[tokio::test]
async fn encrypts_and_decrypts_nip04_with_the_bunkers_key() {
    let s = setup();
    let client = connected_with(&s, &["nip04_encrypt", "nip04_decrypt"]).await;
    let friend = Keys::generate();

    let sent = call(&s, &client, "nip04_encrypt", &friend.public_key(), "hello friend").await.result.expect("encrypted");
    assert_eq!(nip04::decrypt(friend.secret_key(), &s.bunker.public_key(), &sent).unwrap(), "hello friend");
    let incoming = nip04::encrypt(friend.secret_key(), &s.bunker.public_key(), "hello brand").unwrap();
    let read = call(&s, &client, "nip04_decrypt", &friend.public_key(), &incoming).await;
    assert_eq!(read.result.as_deref(), Some("hello brand"), "{:?}", read.error);
}

#[tokio::test]
async fn each_method_needs_its_own_permission_and_a_refusal_decrypts_and_records_nothing() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt"]).await;
    let friend = Keys::generate();
    let incoming = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "secret", nip44::Version::V2).unwrap();

    for method in ["nip44_decrypt", "nip04_encrypt", "nip04_decrypt"] {
        let refused = call(&s, &client, method, &friend.public_key(), &incoming).await;
        assert_eq!(refused.result, None);
        assert_eq!(refused.error.unwrap(), format!("Forbidden: this connection may not call {}", method));
    }
    assert!(decrypt_events(&s).is_empty());
    assert!(call(&s, &client, "nip44_encrypt", &friend.public_key(), "allowed").await.result.is_some());
}

#[tokio::test]
async fn the_client_request_narrows_the_methods_granted() {
    let s = setup();
    let perms: Vec<String> = std::iter::once(sign_permission(1)).chain(ENCRYPTION_METHODS.iter().map(|m| m.to_string())).collect();
    let secret = issue_perms(&s, &perms, 24);
    let client = Keys::generate();
    let mut params = connect_params(&s, &secret);
    params.push("nip44_encrypt,nip44_decrypt".to_string());
    connect(&s, &client, params).await;

    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.perms, vec!["nip44_encrypt".to_string(), "nip44_decrypt".to_string()]);
    let refused = call(&s, &client, "nip04_encrypt", &Keys::generate().public_key(), "x").await;
    assert_eq!(refused.error.as_deref(), Some("Forbidden: this connection may not call nip04_encrypt"));
}

#[tokio::test]
async fn refuses_missing_parameters_and_a_pubkey_that_is_not_hex() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt"]).await;
    let missing = s.handler.handle_request(request("nip44_encrypt", vec![Keys::generate().public_key().to_hex()]), client.public_key()).await;
    assert_eq!(missing.error.as_deref(), Some("Missing parameters: expected a pubkey and a payload"));
    let npub = Keys::generate().public_key().to_bech32().unwrap();
    let not_hex = s.handler.handle_request(request("nip44_decrypt", vec![npub, "x".into()]), client.public_key()).await;
    assert_eq!(not_hex.error.as_deref(), Some("Invalid pubkey"));
    assert!(decrypt_events(&s).is_empty());
}

#[tokio::test]
async fn refuses_a_nip46_app_key_as_the_peer_active_or_not() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"]).await;
    let other_app = connected_with(&s, &[]).await;
    let gone_app = connected_with(&s, &[]).await;
    let gone = s.db.active_nip46_connection(&gone_app.public_key().to_hex()).unwrap().unwrap();
    s.db.revoke_nip46_connection(&gone.id, "logout", Utc::now()).unwrap();

    // What another app sent the bunker, encrypted to the bunker's key.
    let transport = nip44::encrypt(other_app.secret_key(), &s.bunker.public_key(), r#"{"id":"1","method":"sign_event","params":[]}"#, nip44::Version::V2).unwrap();
    for peer in [other_app.public_key(), gone_app.public_key(), client.public_key()] {
        for method in ENCRYPTION_METHODS {
            let refused = call(&s, &client, method, &peer, &transport).await;
            assert_eq!(refused.result, None);
            assert_eq!(refused.error.unwrap(), format!("Forbidden: {} is not allowed with a NIP-46 app key", method));
        }
    }
    assert!(decrypt_events(&s).is_empty());
}

#[tokio::test]
async fn each_decrypt_is_recorded_with_member_connection_app_and_peer_and_no_content() {
    let s = setup();
    let client = connected_with(&s, &["nip44_decrypt", "nip04_decrypt"]).await;
    let friend = Keys::generate();
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();

    let nip44_text = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "first secret", nip44::Version::V2).unwrap();
    let nip04_text = nip04::encrypt(friend.secret_key(), &s.bunker.public_key(), "second secret").unwrap();
    assert!(call(&s, &client, "nip44_decrypt", &friend.public_key(), &nip44_text).await.result.is_some());
    assert!(call(&s, &client, "nip04_decrypt", &friend.public_key(), &nip04_text).await.result.is_some());
    // A payload that does not decrypt is still recorded: the event is written first.
    let failed = call(&s, &client, "nip44_decrypt", &friend.public_key(), "not a ciphertext").await;
    assert!(failed.error.unwrap().starts_with("Decryption error"));

    let events = decrypt_events(&s);
    assert_eq!(events.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["nip44_decrypt", "nip04_decrypt", "nip44_decrypt"]);
    for event in &events {
        assert_eq!(event.member_pubkey.as_deref(), Some(s.member.public_key().to_hex().as_str()));
        assert_eq!(event.member_name.as_deref(), Some("Member"));
        assert_eq!(event.connection_id.as_deref(), Some(connection.id.as_str()));
        assert_eq!(event.token_id.as_deref(), Some(connection.token_id.as_str()));
        assert_eq!(event.client_pubkey.as_deref(), Some(client.public_key().to_hex().as_str()));
        assert_eq!(event.actor_pubkey, None);
        assert_eq!(event.detail.as_deref(), Some(format!("peer={}", friend.public_key().to_hex()).as_str()));
        let row = serde_json::to_string(event).unwrap();
        for content in ["first secret", "second secret", nip44_text.as_str(), nip04_text.as_str()] {
            assert!(!row.contains(content));
        }
    }
}

#[tokio::test]
async fn encrypts_are_not_recorded_and_use_is_touched() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt"]).await;
    assert!(call(&s, &client, "nip44_encrypt", &Keys::generate().public_key(), "x").await.result.is_some());
    assert!(decrypt_events(&s).is_empty());
    assert!(s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap().last_used_at.is_some());
}

#[tokio::test]
async fn a_removed_members_connection_cannot_decrypt() {
    let s = setup();
    let client = connected_with(&s, &["nip44_decrypt"]).await;
    s.db.add_team_member("Admin", &Keys::generate().public_key().to_hex(), Role::Administrator).unwrap();
    let member = s.db.find_member_by_pubkey(&s.member.public_key().to_hex()).unwrap().unwrap();
    s.db.remove_team_member(member.id).unwrap();
    let friend = Keys::generate();
    let incoming = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "x", nip44::Version::V2).unwrap();
    let refused = call(&s, &client, "nip44_decrypt", &friend.public_key(), &incoming).await;
    assert_eq!(refused.error.as_deref(), Some(NOT_CONNECTED));
    assert!(decrypt_events(&s).is_empty());
}

/// A client connected `ago` with a token whose connection lasts `lifetime_hours` (#98), redeemed
/// through the database at that time, as the handler would have then.
fn connected_ago(s: &Setup, lifetime_hours: Option<u32>, ago: Duration) -> (Keys, bunker::db::Nip46Connection) {
    let then = Utc::now() - ago;
    let secret = format!("secret-{}", uuid::Uuid::new_v4());
    let perms = vec![sign_permission(1), "nip44_encrypt".to_string()];
    s.db.create_nip46_token_with_lifetime(&hash_secret(&secret), &s.member.public_key().to_hex(), "admin", "test", &perms, then, then + Duration::hours(24), lifetime_hours).unwrap();
    let client = Keys::generate();
    let connection = s.db.redeem_nip46_token(&hash_secret(&secret), &client.public_key().to_hex(), None, &Default::default(), then).unwrap().unwrap();
    (client, connection)
}

fn expired_events(s: &Setup) -> Vec<bunker::audit::ExportRow> {
    s.db.audit_events().unwrap().into_iter().filter(|e| e.detail.as_deref() == Some("reason=expired")).collect()
}

#[tokio::test]
async fn a_connection_lasts_its_lifetime_from_when_it_connected() {
    let s = setup();
    let (client, connection) = connected_ago(&s, Some(2), Duration::minutes(30));
    let expires_at = connection.expires_at.expect("a lifetime");
    // Two hours after it connected, to the microsecond as stored.
    assert_eq!(expires_at, (connection.connected_at + Duration::hours(2)).trunc_subsecs(6));
    assert!(sign(&s, &client, Kind::TextNote).await.result.is_some());
    assert_eq!(s.handler.connection_count().await, 1);
}

#[tokio::test]
async fn an_expired_connection_is_refused_once_as_expired_then_as_not_connected() {
    let s = setup();
    let (client, connection) = connected_ago(&s, Some(1), Duration::hours(2));
    let friend = Keys::generate().public_key();

    // Refused before anything marks it: active connections exclude it, and the count too.
    assert!(s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().is_none());
    assert_eq!(s.handler.connection_count().await, 0);

    let first = sign(&s, &client, Kind::TextNote).await;
    assert_eq!(first.result, None);
    assert_eq!(first.error.as_deref(), Some(EXPIRED));
    let again = s.handler.handle_request(request("nip44_encrypt", vec![friend.to_hex(), "x".into()]), client.public_key()).await;
    assert_eq!(again.error.as_deref(), Some(NOT_CONNECTED));

    // Marked expired, at its expiry rather than when it was noticed, by no one, and recorded once.
    let ended = s.db.list_nip46_connections(None).unwrap().into_iter().find(|c| c.id == connection.id).unwrap();
    assert_eq!(ended.revoked_reason.as_deref(), Some("expired"));
    assert_eq!(ended.revoked_at, connection.expires_at);
    assert_eq!(ended.revoked_by, None);
    let events = expired_events(&s);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "connection_revoked");
    assert_eq!(events[0].actor_pubkey, None);
    assert_eq!(events[0].connection_id.as_deref(), Some(connection.id.as_str()));
    assert_eq!(events[0].member_pubkey.as_deref(), Some(s.member.public_key().to_hex().as_str()));
    assert_eq!(events[0].client_pubkey.as_deref(), Some(client.public_key().to_hex().as_str()));
}

#[tokio::test]
async fn the_sweep_ends_only_expired_connections() {
    let s = setup();
    let (_, expired) = connected_ago(&s, Some(1), Duration::hours(3));
    let (_, current) = connected_ago(&s, Some(24), Duration::hours(3));
    let (_, unlimited) = connected_ago(&s, None, Duration::days(400));

    let ended = bunker::nip46::expire_connections(&s.db);
    assert_eq!(ended.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec![expired.id.as_str()]);
    // Running it again ends nothing more, and records nothing more.
    assert!(bunker::nip46::expire_connections(&s.db).is_empty());
    assert_eq!(expired_events(&s).len(), 1);

    let active: Vec<String> = s.db.list_nip46_connections(None).unwrap().into_iter().filter(|c| c.revoked_at.is_none()).map(|c| c.id).collect();
    assert!(active.contains(&current.id) && active.contains(&unlimited.id));
    assert_eq!(active.len(), 2);
}

#[tokio::test]
async fn a_token_without_a_lifetime_connects_as_before() {
    let s = setup();
    let client = Keys::generate();
    let secret = issue(&s, &[1], 24);
    connect(&s, &client, connect_params(&s, &secret)).await;
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.expires_at, None);
    let tokens = s.db.list_nip46_tokens().unwrap();
    assert_eq!(tokens[0].connection_lifetime_hours, None);
    assert!(s.db.active_nip46_connection_at(&client.public_key().to_hex(), Utc::now() + Duration::days(3650)).unwrap().is_some());
}

#[tokio::test]
async fn an_app_that_reconnects_after_expiry_has_its_old_connection_marked_expired_not_replaced() {
    let s = setup();
    let (client, old) = connected_ago(&s, Some(1), Duration::hours(2));
    let secret = issue(&s, &[1], 24);
    let connected = connect(&s, &client, connect_params(&s, &secret)).await;
    assert_eq!(connected.result.as_deref(), Some("ack"));
    let ended = s.db.list_nip46_connections(None).unwrap().into_iter().find(|c| c.id == old.id).unwrap();
    assert_eq!(ended.revoked_reason.as_deref(), Some("expired"));
}

#[tokio::test]
async fn the_lifetime_and_expiry_are_in_the_audit_details() {
    let s = setup();
    let (_, connection) = connected_ago(&s, Some(5), Duration::minutes(1));
    let events = s.db.audit_events().unwrap();
    let issued = events.iter().find(|e| e.action == "token_issued").unwrap();
    assert!(issued.detail.as_deref().unwrap().ends_with(";connection_lifetime_hours=5"), "{:?}", issued.detail);
    let created = events.iter().find(|e| e.action == "connection_created").unwrap();
    let expiry = bunker::audit::db_time(connection.expires_at.unwrap());
    assert_eq!(created.detail.as_deref(), Some(format!("perms=sign_event:1,nip44_encrypt;expires_at={}", expiry).as_str()));
}
