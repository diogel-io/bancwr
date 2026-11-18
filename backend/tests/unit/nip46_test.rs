// NIP-46 behind connection tokens (#53): every refusal path, the grant intersection, logout,
// persistence across a reopen, and member removal ending a connection. Plus the encryption
// methods (#97): their permissions, parameters, the refusal of NIP-46 app keys as a peer, and the
// audit event each decrypt leaves before it happens.
use bunker::db::{Database, TokenOptions};
use bunker::rate_limit::RateLimit;
use bunker::approvals::Scheme;
use bunker::nip46::{hash_secret, sign_permission, ApprovalError, Nip46Handler, Nip46Outcome, Nip46Request, Nip46Response, ENCRYPTION_METHODS, EXPIRED, NOT_CONNECTED, TOKEN_USED_UP, TOO_MANY_NEW_SENDERS};
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

    // Asking only for what the token does not allow connects nothing, and uses the token up: a
    // retry asking for what it allows is refused as used.
    let other = issue(&s, &[1], 24);
    let mut params = connect_params(&s, &other);
    params.push("sign_event:4".to_string());
    let refused = connect(&s, &Keys::generate(), params).await;
    assert_eq!(
        refused.error.as_deref(),
        Some(format!("None of the requested permissions are allowed by this connection token{}", TOKEN_USED_UP).as_str())
    );
    let mut retry = connect_params(&s, &other);
    retry.push(String::new());
    assert_eq!(connect(&s, &Keys::generate(), retry).await.error.as_deref(), Some("This connection token has already been used"));
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

/// Asserts every encryption method naming `peer` is refused for `client`, and nothing is decrypted.
async fn assert_refused_as_peer(s: &Setup, client: &Keys, peer: &Keys) {
    let transport = nip44::encrypt(peer.secret_key(), &s.bunker.public_key(), r#"{"id":"1","method":"connect","params":[]}"#, nip44::Version::V2).unwrap();
    for method in ENCRYPTION_METHODS {
        let refused = call(s, client, method, &peer.public_key(), &transport).await;
        assert_eq!(refused.result, None, "{}", method);
        assert_eq!(refused.error.unwrap(), format!("Forbidden: {} is not allowed with a NIP-46 app key", method));
    }
    assert!(decrypt_events(s).is_empty());
}

#[tokio::test]
async fn refuses_as_the_peer_an_app_whose_connect_was_refused() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"]).await;
    let refused_app = Keys::generate();
    let secret = issue(&s, &[1], 24);
    let mut params = connect_params(&s, &secret);
    params.push("sign_event:4".to_string());
    assert!(connect(&s, &refused_app, params).await.error.is_some());
    assert!(!s.db.list_nip46_connections(None).unwrap().iter().any(|c| c.client_pubkey == refused_app.public_key().to_hex()), "it never connected");

    assert_refused_as_peer(&s, &client, &refused_app).await;
    // And its token is used up, so the right parameters do not connect it later either.
    assert_eq!(connect(&s, &Keys::generate(), connect_params(&s, &secret)).await.error.as_deref(), Some("This connection token has already been used"));
}

#[tokio::test]
async fn refuses_as_the_peer_a_key_that_sent_any_request_without_connecting() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"]).await;
    let pinger = Keys::generate();
    assert_eq!(s.handler.handle_request(request("ping", vec![]), pinger.public_key()).await.error.as_deref(), Some(NOT_CONNECTED));
    let wrong_secret = Keys::generate();
    assert!(connect(&s, &wrong_secret, connect_params(&s, "wrong")).await.error.is_some());
    let elsewhere = Keys::generate();
    assert!(connect(&s, &elsewhere, vec![Keys::generate().public_key().to_hex(), "x".into()]).await.error.is_some());

    for peer in [&pinger, &wrong_secret, &elsewhere] {
        assert_refused_as_peer(&s, &client, peer).await;
    }
}

#[tokio::test]
async fn never_decrypts_a_nip46_message_even_from_a_key_the_bunker_has_not_seen() {
    let s = setup();
    let client = connected_with(&s, &["nip44_decrypt", "nip04_decrypt"]).await;
    // A connect the bunker never received, such as one sent while it was stopped.
    let unseen = Keys::generate();
    let connect = serde_json::json!({ "id": "1", "method": "connect", "params": [s.bunker.public_key().to_hex(), "a-secret"] }).to_string();
    let response = serde_json::json!({ "id": "2", "result": "ack" }).to_string();
    for plaintext in [&connect, &response] {
        let nip44_text = nip44::encrypt(unseen.secret_key(), &s.bunker.public_key(), plaintext, nip44::Version::V2).unwrap();
        let refused = call(&s, &client, "nip44_decrypt", &unseen.public_key(), &nip44_text).await;
        assert!(refused.result.is_none());
        assert_eq!(refused.error.as_deref(), Some("Forbidden: nip44_decrypt does not return NIP-46 messages"));
        let nip04_text = nip04::encrypt(unseen.secret_key(), &s.bunker.public_key(), plaintext).unwrap();
        let refused = call(&s, &client, "nip04_decrypt", &unseen.public_key(), &nip04_text).await;
        assert!(refused.result.is_none());
        assert!(!refused.error.unwrap_or_default().contains("a-secret"));
    }
    // Other JSON still decrypts.
    let other = r#"{"id":"3","kind":1}"#;
    let text = nip44::encrypt(unseen.secret_key(), &s.bunker.public_key(), other, nip44::Version::V2).unwrap();
    assert_eq!(call(&s, &client, "nip44_decrypt", &unseen.public_key(), &text).await.result.as_deref(), Some(other));
}

#[tokio::test]
async fn the_bunkers_own_key_and_a_key_that_never_sent_a_request_still_work_as_the_peer() {
    let s = setup();
    let client = connected_with(&s, &["nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"]).await;
    // Other keys have sent requests, which changes nothing for these.
    s.handler.handle_request(request("ping", vec![]), Keys::generate().public_key()).await;
    let friend = Keys::generate();

    let incoming = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "hello brand", nip44::Version::V2).unwrap();
    assert_eq!(call(&s, &client, "nip44_decrypt", &friend.public_key(), &incoming).await.result.as_deref(), Some("hello brand"));
    let incoming = nip04::encrypt(friend.secret_key(), &s.bunker.public_key(), "hello again").unwrap();
    assert_eq!(call(&s, &client, "nip04_decrypt", &friend.public_key(), &incoming).await.result.as_deref(), Some("hello again"));
    assert!(call(&s, &client, "nip04_encrypt", &friend.public_key(), "x").await.result.is_some());

    let own = call(&s, &client, "nip44_encrypt", &s.bunker.public_key(), "private list").await.result.unwrap();
    assert_eq!(call(&s, &client, "nip44_decrypt", &s.bunker.public_key(), &own).await.result.as_deref(), Some("private list"));
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

// Rate limits (#101).

/// A client connected through the handler with a token carrying `rate_limit`, allowed kind 1 and
/// every encryption method.
async fn connected_limited(s: &Setup, rate_limit: Option<RateLimit>) -> Keys {
    let secret = format!("secret-{}", uuid::Uuid::new_v4());
    let now = Utc::now();
    let perms: Vec<String> = std::iter::once(sign_permission(1)).chain(ENCRYPTION_METHODS.iter().map(|m| m.to_string())).collect();
    let options = TokenOptions { rate_limit, ..TokenOptions::default() };
    s.db.create_nip46_token_with(&hash_secret(&secret), &s.member.public_key().to_hex(), "admin", "test", &perms, now, now + Duration::hours(1), options).unwrap();
    let client = Keys::generate();
    let connected = connect(s, &client, connect_params(s, &secret)).await;
    assert_eq!(connected.result.as_deref(), Some("ack"), "{:?}", connected.error);
    client
}

/// One a minute with a burst of 2: the third request at once is refused, and nothing refills
/// during a test.
const TIGHT: RateLimit = RateLimit { per_minute: 1, burst: 2 };

fn rate_limited_events(s: &Setup) -> Vec<bunker::audit::ExportRow> {
    s.db.audit_events().unwrap().into_iter().filter(|e| e.action == "connection_rate_limited").collect()
}

#[tokio::test]
async fn signing_beyond_the_burst_is_refused_with_when_to_try_again_and_signs_nothing() {
    let s = setup();
    let client = connected_limited(&s, Some(TIGHT)).await;
    for _ in 0..2 {
        assert!(sign(&s, &client, Kind::TextNote).await.result.is_some());
    }
    let signatures = s.db.signature_count().unwrap();
    let refused = sign(&s, &client, Kind::TextNote).await;
    assert_eq!(refused.result, None);
    let error = refused.error.unwrap();
    assert!(error.starts_with("Rate limited: this connection may make 1 requests a minute (burst 2). Try again in "), "{}", error);
    assert!(error.ends_with(" s"), "{}", error);
    assert_eq!(s.db.signature_count().unwrap(), signatures, "nothing signed or logged");
    assert_eq!(s.handler.rate_limited_count(), 1);
}

#[tokio::test]
async fn a_decrypt_beyond_the_limit_is_refused_before_it_is_recorded() {
    let s = setup();
    let client = connected_limited(&s, Some(TIGHT)).await;
    let friend = Keys::generate();
    let incoming = nip44::encrypt(friend.secret_key(), &s.bunker.public_key(), "secret", nip44::Version::V2).unwrap();
    for _ in 0..2 {
        assert!(call(&s, &client, "nip44_decrypt", &friend.public_key(), &incoming).await.result.is_some());
    }
    let refused = call(&s, &client, "nip44_decrypt", &friend.public_key(), &incoming).await;
    assert!(refused.error.unwrap().starts_with("Rate limited"));
    assert_eq!(decrypt_events(&s).len(), 2, "the refused decrypt left no decrypt event");
}

#[tokio::test]
async fn a_refusal_for_a_kind_not_granted_still_counts() {
    let s = setup();
    let client = connected_limited(&s, Some(TIGHT)).await;
    for _ in 0..2 {
        assert!(sign(&s, &client, Kind::Metadata).await.error.unwrap().starts_with("Forbidden"));
    }
    assert!(sign(&s, &client, Kind::Metadata).await.error.unwrap().starts_with("Rate limited"));
}

#[tokio::test]
async fn the_methods_that_do_not_use_the_key_still_answer_when_limited() {
    let s = setup();
    let client = connected_limited(&s, Some(TIGHT)).await;
    for _ in 0..3 {
        sign(&s, &client, Kind::TextNote).await;
    }
    for _ in 0..30 {
        let ping = s.handler.handle_request(request("ping", vec![]), client.public_key()).await;
        assert_eq!(ping.result.as_deref(), Some("pong"));
    }
    let key = s.handler.handle_request(request("get_public_key", vec![]), client.public_key()).await;
    assert_eq!(key.result, Some(s.bunker.public_key().to_hex()));
    let logout = s.handler.handle_request(request("logout", vec![]), client.public_key()).await;
    assert_eq!(logout.result.as_deref(), Some("ack"));
}

#[tokio::test]
async fn another_connection_is_unaffected() {
    let s = setup();
    let busy = connected_limited(&s, Some(TIGHT)).await;
    let quiet = connected_limited(&s, Some(TIGHT)).await;
    for _ in 0..5 {
        sign(&s, &busy, Kind::TextNote).await;
    }
    assert!(sign(&s, &busy, Kind::TextNote).await.error.unwrap().starts_with("Rate limited"));
    assert!(sign(&s, &quiet, Kind::TextNote).await.result.is_some());
}

#[tokio::test]
async fn a_connection_without_a_limit_gets_the_default() {
    let s = setup();
    let client = connected_limited(&s, None).await;
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.rate_limit, None);
    assert_eq!(connection.effective_rate_limit(), RateLimit::DEFAULT);
    // The default burst is 20. Refilling at one a second may add one more while these run.
    let mut allowed = 0;
    for _ in 0..25 {
        if sign(&s, &client, Kind::TextNote).await.result.is_some() {
            allowed += 1;
        }
    }
    assert!((20..=21).contains(&allowed), "{}", allowed);
    assert!(s.handler.rate_limited_count() >= 4);
}

#[tokio::test]
async fn the_connection_keeps_its_tokens_limit() {
    let s = setup();
    let client = connected_limited(&s, Some(RateLimit { per_minute: 30, burst: 10 })).await;
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    assert_eq!(connection.rate_limit, Some(RateLimit { per_minute: 30, burst: 10 }));
}

#[tokio::test]
async fn refusals_are_audited_once_a_minute_however_many_there_are() {
    let s = setup();
    let client = connected_limited(&s, Some(TIGHT)).await;
    for _ in 0..12 {
        sign(&s, &client, Kind::TextNote).await;
    }
    assert_eq!(s.handler.rate_limited_count(), 10);
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    let events = rate_limited_events(&s);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.connection_id.as_deref(), Some(connection.id.as_str()));
    assert_eq!(event.member_pubkey.as_deref(), Some(s.member.public_key().to_hex().as_str()));
    assert_eq!(event.client_pubkey.as_deref(), Some(client.public_key().to_hex().as_str()));
    assert_eq!(event.actor_pubkey, None);
    assert_eq!(event.detail.as_deref(), Some("rate_per_minute=1;burst=2"));
}

#[tokio::test]
async fn traffic_within_the_limit_is_unchanged() {
    let s = setup();
    let client = connected_limited(&s, Some(RateLimit { per_minute: 600, burst: 100 })).await;
    for _ in 0..50 {
        assert!(sign(&s, &client, Kind::TextNote).await.result.is_some());
    }
    assert_eq!(s.handler.rate_limited_count(), 0);
    assert!(rate_limited_events(&s).is_empty());
}

#[tokio::test]
async fn a_token_with_a_limit_records_it_when_issued_and_the_default_does_not() {
    let s = setup();
    connected_limited(&s, Some(TIGHT)).await;
    connected_limited(&s, Some(RateLimit::DEFAULT)).await;
    let issued: Vec<String> = s.db.audit_events().unwrap().into_iter().filter(|e| e.action == "token_issued").filter_map(|e| e.detail).collect();
    assert_eq!(issued.len(), 2);
    assert!(issued[0].ends_with(";rate_per_minute=1;rate_burst=2"), "{}", issued[0]);
    assert!(!issued[1].contains("rate_"), "{}", issued[1]);
}

// Approval before signing (#102).

/// A client connected through the handler with a token allowing kinds 1 and 7, of which
/// `approval` need approval.
async fn connected_gated(s: &Setup, approval: &[u16]) -> Keys {
    connected_gated_with(s, approval, None).await
}

async fn connected_gated_with(s: &Setup, approval: &[u16], rate_limit: Option<RateLimit>) -> Keys {
    let secret = format!("secret-{}", uuid::Uuid::new_v4());
    let now = Utc::now();
    let perms = vec![sign_permission(1), sign_permission(7)];
    let options = TokenOptions { approval_perms: approval.iter().map(|k| sign_permission(*k)).collect(), rate_limit, ..TokenOptions::default() };
    s.db.create_nip46_token_with(&hash_secret(&secret), &s.member.public_key().to_hex(), "admin", "test", &perms, now, now + Duration::hours(1), options).unwrap();
    let client = Keys::generate();
    let connected = connect(s, &client, connect_params(s, &secret)).await;
    assert_eq!(connected.result.as_deref(), Some("ack"), "{:?}", connected.error);
    client
}

fn approval_events(s: &Setup) -> Vec<bunker::audit::ExportRow> {
    s.db.audit_events().unwrap().into_iter().filter(|e| e.action.starts_with("approval_")).collect()
}

const ADMIN: &str = "aa00000000000000000000000000000000000000000000000000000000000001";

#[tokio::test]
async fn a_kind_that_needs_approval_is_held_and_not_signed() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    let held = sign(&s, &client, Kind::TextNote).await;
    assert_eq!(held.result.as_deref(), Some("auth_url"));
    assert_eq!(s.db.signature_count().unwrap(), 0);

    let pending = s.handler.pending_approvals(None);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind(), 1);
    assert_eq!(pending[0].event.content, "hello");
    assert_eq!(pending[0].event.pubkey, s.bunker.public_key(), "previewed as it will be signed");
    assert_eq!(held.error, Some(format!("/approvals/{}", pending[0].id)));
    assert_eq!(s.handler.approvals_pending(), 1);

    let events = approval_events(&s);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "approval_requested");
    assert_eq!(events[0].detail.as_deref(), Some(pending[0].audit_detail().as_str()));
    assert_eq!(events[0].actor_pubkey, None);
}

#[tokio::test]
async fn a_kind_that_does_not_need_approval_signs_at_once() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    assert!(sign(&s, &client, Kind::Reaction).await.result.unwrap().contains("\"sig\""));
    assert!(s.handler.pending_approvals(None).is_empty());
}

#[tokio::test]
async fn the_auth_challenge_points_at_the_console_when_its_address_is_known() {
    let s = setup();
    s.handler.configure_approvals(Duration::minutes(30), Some("https://bancwr.example".to_string()));
    let client = connected_gated(&s, &[1]).await;
    let unsigned = EventBuilder::new(Kind::TextNote, "x").build(client.public_key());
    let outcome = s.handler.handle(request("sign_event", vec![serde_json::to_string(&unsigned).unwrap()]), client.public_key(), Scheme::Nip04).await;
    let Nip46Outcome::Held { pending_id, notice } = outcome else { panic!("held") };
    let notice = notice.expect("an auth challenge");
    assert_eq!(notice.id, "sign_event-id");
    assert_eq!(notice.error, Some(format!("https://bancwr.example/approvals/{}", pending_id)));
    assert_eq!(s.handler.pending_approvals(None)[0].scheme, Scheme::Nip04, "answered later as it came");

    // Without the console's address, nothing is sent now.
    s.handler.configure_approvals(Duration::minutes(30), None);
    let outcome = s.handler.handle(request("sign_event", vec![serde_json::to_string(&unsigned).unwrap()]), client.public_key(), Scheme::Nip44).await;
    assert!(matches!(outcome, Nip46Outcome::Held { notice: None, .. }));
}

#[tokio::test]
async fn approving_signs_exactly_the_event_previewed_and_records_who() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    sign(&s, &client, Kind::TextNote).await;
    let pending = s.handler.pending_approvals(None).remove(0);

    let reply = s.handler.approve(pending.id, &pending.event_id.to_hex(), Some(ADMIN), Some("Looks good")).await.unwrap();
    assert_eq!(reply.client_pubkey, client.public_key());
    assert_eq!(reply.response.id, "sign_event-id");
    let event: Event = serde_json::from_str(reply.response.result.as_deref().expect("signed")).unwrap();
    assert_eq!(event.id, pending.event_id);
    assert_eq!(event.pubkey, s.bunker.public_key());
    assert!(event.verify().is_ok());
    assert_eq!(s.db.signature_count().unwrap(), 1);
    assert!(s.handler.pending_approvals(None).is_empty());

    let approved = approval_events(&s).into_iter().find(|e| e.action == "approval_approved").unwrap();
    assert_eq!(approved.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(approved.detail, Some(format!("{};note=Looks good", pending.audit_detail())));
    // Decided once.
    assert!(matches!(s.handler.approve(pending.id, &pending.event_id.to_hex(), Some(ADMIN), None).await, Err(ApprovalError::NotFound)));
}

#[tokio::test]
async fn approving_another_event_than_the_one_waiting_is_refused_and_it_still_waits() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    sign(&s, &client, Kind::TextNote).await;
    let pending = s.handler.pending_approvals(None).remove(0);
    let other = EventId::all_zeros().to_hex();
    assert!(matches!(s.handler.approve(pending.id, &other, Some(ADMIN), None).await, Err(ApprovalError::EventMismatch)));
    assert_eq!(s.db.signature_count().unwrap(), 0);
    assert_eq!(s.handler.pending_approvals(None).len(), 1);
}

#[tokio::test]
async fn rejecting_signs_nothing_and_tells_the_app_with_the_note() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    sign(&s, &client, Kind::TextNote).await;
    let pending = s.handler.pending_approvals(None).remove(0);

    let reply = s.handler.reject(pending.id, Some(ADMIN), Some("Not on brand")).unwrap();
    assert_eq!(reply.response.result, None);
    assert_eq!(reply.response.error.as_deref(), Some("Rejected by an administrator: Not on brand"));
    assert_eq!(s.db.signature_count().unwrap(), 0);
    let rejected = approval_events(&s).into_iter().find(|e| e.action == "approval_rejected").unwrap();
    assert_eq!(rejected.actor_pubkey.as_deref(), Some(ADMIN));
    assert_eq!(rejected.detail, Some(format!("{};note=Not on brand", pending.audit_detail())));
    assert!(matches!(s.handler.reject(pending.id, Some(ADMIN), None), Err(ApprovalError::NotFound)));
}

#[tokio::test]
async fn a_request_nobody_decides_times_out_and_signs_nothing() {
    let s = setup();
    s.handler.configure_approvals(Duration::minutes(10), None);
    let client = connected_gated(&s, &[1]).await;
    sign(&s, &client, Kind::TextNote).await;
    let pending = s.handler.pending_approvals(None).remove(0);

    assert!(s.handler.sweep_approvals(Utc::now() + Duration::minutes(9)).is_empty());
    let replies = s.handler.sweep_approvals(Utc::now() + Duration::minutes(11));
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].response.error.as_deref(), Some("Approval timed out after 10 minutes: nothing was signed"));
    assert_eq!(s.db.signature_count().unwrap(), 0);
    assert!(s.handler.pending_approvals(None).is_empty());
    let expired = approval_events(&s).into_iter().find(|e| e.action == "approval_expired").unwrap();
    assert_eq!(expired.actor_pubkey, None);
    assert_eq!(expired.detail, Some(format!("{};timeout_seconds=600", pending.audit_detail())));
}

#[tokio::test]
async fn a_request_whose_connection_ends_is_cancelled_by_the_sweep_and_by_approving() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    sign(&s, &client, Kind::TextNote).await;
    sign(&s, &client, Kind::TextNote).await;
    let connection = s.db.active_nip46_connection(&client.public_key().to_hex()).unwrap().unwrap();
    let first = s.handler.pending_approvals(None).remove(0);
    s.db.revoke_nip46_connection(&connection.id, "revoked", Utc::now()).unwrap();

    // Approving one finds the connection gone: cancelled, and the app told.
    let Err(ApprovalError::ConnectionEnded(reply)) = s.handler.approve(first.id, &first.event_id.to_hex(), Some(ADMIN), None).await else {
        panic!("cancelled")
    };
    assert_eq!(reply.response.error.as_deref(), Some("This connection ended: nothing was signed"));
    // The sweep cancels the other.
    let replies = s.handler.sweep_approvals(Utc::now());
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].response.error.as_deref(), Some("This connection ended: nothing was signed"));
    assert_eq!(s.db.signature_count().unwrap(), 0);
    let cancelled: Vec<_> = approval_events(&s).into_iter().filter(|e| e.action == "approval_cancelled").collect();
    assert_eq!(cancelled.len(), 2);
    assert!(cancelled.iter().all(|e| e.detail.as_deref().unwrap().ends_with(";reason=connection_ended")));
}

#[tokio::test]
async fn too_many_requests_waiting_for_one_connection_are_refused() {
    let s = setup();
    // A rate limit above the queue's bound, so the bound is what refuses.
    let gated = connected_gated_with(&s, &[1], Some(RateLimit { per_minute: 600, burst: 100 })).await;
    for _ in 0..bunker::approvals::MAX_PENDING_PER_CONNECTION {
        assert_eq!(sign(&s, &gated, Kind::TextNote).await.result.as_deref(), Some("auth_url"));
    }
    let refused = sign(&s, &gated, Kind::TextNote).await;
    assert_eq!(refused.error.as_deref(), Some("Too many requests are waiting for approval"));
    assert_eq!(s.handler.approvals_pending(), bunker::approvals::MAX_PENDING_PER_CONNECTION);
}

#[tokio::test]
async fn held_requests_count_against_the_rate_limit() {
    let s = setup();
    let gated = connected_gated_with(&s, &[1], Some(RateLimit { per_minute: 1, burst: 2 })).await;
    for _ in 0..2 {
        assert_eq!(sign(&s, &gated, Kind::TextNote).await.result.as_deref(), Some("auth_url"));
    }
    assert!(sign(&s, &gated, Kind::TextNote).await.error.unwrap().starts_with("Rate limited"));
    assert_eq!(s.handler.approvals_pending(), 2);
}

#[tokio::test]
async fn a_kind_not_granted_is_refused_rather_than_held() {
    let s = setup();
    let client = connected_gated(&s, &[1]).await;
    let refused = sign(&s, &client, Kind::Metadata).await;
    assert!(refused.error.unwrap().starts_with("Forbidden"));
    assert!(s.handler.pending_approvals(None).is_empty());
}

#[tokio::test]
async fn past_the_hourly_limit_of_new_senders_a_new_app_is_refused_and_a_known_one_answered() {
    use bunker::db::NEW_NIP46_SENDERS_PER_HOUR;
    let s = setup();
    let known = Keys::generate();
    let secret = issue(&s, &[1], 24);
    assert_eq!(connect(&s, &known, connect_params(&s, &secret)).await.result.as_deref(), Some("ack"));
    for _ in 1..NEW_NIP46_SENDERS_PER_HOUR {
        assert!(s.db.record_nip46_sender(&Keys::generate().public_key().to_hex(), Utc::now()).unwrap());
    }
    // A new key is refused before its request is handled: its token stays unused.
    let fresh = issue(&s, &[1], 24);
    let refused = connect(&s, &Keys::generate(), connect_params(&s, &fresh)).await;
    assert_eq!(refused.error.as_deref(), Some(TOO_MANY_NEW_SENDERS));
    assert_eq!(s.db.list_nip46_tokens().unwrap().iter().filter(|t| t.used_at.is_none()).count(), 1);
    // An app already known is still answered.
    assert!(sign(&s, &known, Kind::TextNote).await.result.unwrap().contains("\"sig\""));
}
