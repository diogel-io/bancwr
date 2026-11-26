// An identity's profile, follows and relays edited in the console (#150), through
// `POST /api/bunker/identities/:pubkey/sign`: the bunker signs kinds 0, 3 and 10002 and a Blossom
// upload authorisation as the identity, for an administrator, and logs each under them, marked as
// the console's. Anything else is refused and nothing is signed or logged.
#[path = "../common/mod.rs"]
mod common;

use bunker::registry::Role;
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn sign(app: &common::TestApp, identity: &str, as_member: &str, template: Value) -> reqwest::Response {
    app.signed(Method::POST, &format!("/api/bunker/identities/{identity}/sign"), as_member).json(&template).send().await.unwrap()
}

async fn logs(app: &common::TestApp) -> Vec<Value> {
    app.signed(Method::GET, "/api/bunker/logs", &app.admin.public_key().to_hex()).send().await.unwrap().json().await.unwrap()
}

async fn export(app: &common::TestApp) -> Vec<Value> {
    let day = chrono::Utc::now().format("%Y-%m-%d");
    let path = format!("/api/bunker/logs/export?format=jsonl&from={day}&to={day}");
    let text = app.signed(Method::GET, &path, &app.admin.public_key().to_hex()).send().await.unwrap().text().await.unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn upload_auth(expires_in: i64) -> Value {
    let at = now();
    json!({
        "kind": 24242,
        "created_at": at,
        "content": "Upload profile image",
        "tags": [["t", "upload"], ["x", "a".repeat(64)], ["expiration", (at + expires_in).to_string()]]
    })
}

#[tokio::test]
async fn an_administrator_has_an_identity_sign_its_profile_follows_and_relays() {
    let sales = Keys::generate();
    let app = common::spawn_with_identities(true, None, &[("Sales", sales.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    let followed = Keys::generate().public_key().to_hex();

    let templates = [
        json!({ "kind": 0, "created_at": now(), "tags": [], "content": "{\"name\":\"Sales\"}" }),
        // A second ahead, as the editors set it past the event they replace.
        json!({ "kind": 3, "created_at": now() + 60, "tags": [["p", followed]], "content": "" }),
        json!({ "kind": 10002, "created_at": now(), "tags": [["r", "wss://relay.example"], ["r", "wss://write.example", "write"]], "content": "" }),
        upload_auth(300),
    ];
    let mut ids = Vec::new();
    for template in templates {
        let res = sign(&app, &sales.public_key().to_bech32().unwrap(), &admin, template.clone()).await;
        assert_eq!(res.status(), StatusCode::OK, "{template}");
        let event: Event = res.json().await.unwrap();
        assert_eq!(event.pubkey, sales.public_key());
        assert!(event.verify().is_ok());
        assert_eq!(u64::from(event.kind.as_u16()), template["kind"].as_u64().unwrap());
        assert_eq!(event.content, template["content"].as_str().unwrap());
        assert_eq!(event.created_at.as_u64() as i64, template["created_at"].as_i64().unwrap());
        assert_eq!(serde_json::to_value(&event.tags).unwrap(), template["tags"]);
        ids.push(event.id.to_hex());
    }

    // The primary's too.
    let primary = sign(&app, &app.bunker.public_key().to_hex(), &admin, json!({ "kind": 0, "created_at": now(), "tags": [], "content": "{}" })).await;
    assert_eq!(primary.status(), StatusCode::OK);
    let primary: Event = primary.json().await.unwrap();
    assert_eq!(primary.pubkey, app.bunker.public_key());

    // Each is in the signing log under the administrator, marked as the console's, with no app.
    let logged = logs(&app).await;
    assert_eq!(logged.len(), 5);
    for id in &ids {
        let entry = logged.iter().find(|l| l["event_id"] == *id).expect("logged");
        assert_eq!(entry["member_pubkey"], admin);
        assert_eq!(entry["pubkey"], admin);
        assert_eq!(entry["origin"], "console");
        assert_eq!(entry["identity_pubkey"], sales.public_key().to_hex());
        assert!(entry["connection_id"].is_null());
    }
    let entry = logged.iter().find(|l| l["event_id"] == primary.id.to_hex()).unwrap();
    assert_eq!(entry["identity_pubkey"], app.bunker.public_key().to_hex());

    // And in the export, with the administrator as the actor, the console as the detail, and no app.
    let exported = export(&app).await;
    let row = exported.iter().find(|r| r["event_id"] == ids[0]).expect("exported");
    assert_eq!(row["action"], "signed");
    assert_eq!(row["actor_pubkey"], admin);
    assert_eq!(row["detail"], "via=console");
    assert!(row["client_pubkey"].is_null(), "no app asked for it: {row}");
    assert_eq!(row["identity_pubkey"], sales.public_key().to_hex());
}

#[tokio::test]
async fn anything_but_those_kinds_and_a_short_upload_authorisation_is_refused_and_not_logged() {
    let sales = Keys::generate();
    let app = common::spawn_with_identities(true, None, &[("Sales", sales.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    let key = sales.public_key().to_hex();
    let at = now();
    let hash = "a".repeat(64);

    let refused: Vec<(Value, &str)> = vec![
        (json!({ "kind": 1, "created_at": at, "tags": [], "content": "a note" }), "kind_not_allowed"),
        (json!({ "kind": 4, "created_at": at, "tags": [], "content": "" }), "kind_not_allowed"),
        (json!({ "kind": 5, "created_at": at, "tags": [], "content": "" }), "kind_not_allowed"),
        (json!({ "kind": 27235, "created_at": at, "tags": [], "content": "" }), "kind_not_allowed"),
        (json!({ "kind": 0, "created_at": at - 3600, "tags": [], "content": "{}" }), "bad_created_at"),
        (json!({ "kind": 0, "created_at": at + 3600, "tags": [], "content": "{}" }), "bad_created_at"),
        (json!({ "kind": 0, "created_at": at, "tags": [], "content": "x".repeat(70 * 1024) }), "too_large"),
        (json!({ "kind": 0, "created_at": at, "tags": [], "content": "not json" }), "bad_content"),
        (json!({ "kind": 0, "created_at": at, "tags": [], "content": "[1]" }), "bad_content"),
        (json!({ "kind": 3, "created_at": at, "tags": [[]], "content": "" }), "bad_tags"),
        (json!({ "kind": 3, "created_at": at, "tags": [["p", "x".repeat(2000)]], "content": "" }), "bad_tags"),
        // Blossom: an upload only, one blob, expiring within 6 minutes.
        (upload_auth(3600), "bad_upload_auth"),
        (upload_auth(-10), "bad_upload_auth"),
        (json!({ "kind": 24242, "created_at": at, "content": "", "tags": [["x", hash], ["expiration", (at + 60).to_string()]] }), "bad_upload_auth"),
        (json!({ "kind": 24242, "created_at": at, "content": "", "tags": [["t", "delete"], ["x", hash], ["expiration", (at + 60).to_string()]] }), "bad_upload_auth"),
        (json!({ "kind": 24242, "created_at": at, "content": "", "tags": [["t", "upload"], ["t", "list"], ["x", hash], ["expiration", (at + 60).to_string()]] }), "bad_upload_auth"),
        (json!({ "kind": 24242, "created_at": at, "content": "", "tags": [["t", "upload"], ["expiration", (at + 60).to_string()]] }), "bad_upload_auth"),
        (json!({ "kind": 24242, "created_at": at, "content": "", "tags": [["t", "upload"], ["x", hash]] }), "bad_upload_auth"),
    ];
    for (template, code) in refused {
        let res = sign(&app, &key, &admin, template.clone()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{template}");
        let body: Value = res.json().await.unwrap();
        assert_eq!(body["error"], code, "{template}");
    }

    // Only a template: a pubkey, id or signature is not accepted.
    let with_pubkey = json!({ "kind": 0, "created_at": at, "tags": [], "content": "{}", "pubkey": admin });
    assert_eq!(sign(&app, &key, &admin, with_pubkey).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // An identity that is not this bunker's, or a member's key, is not found.
    let good = json!({ "kind": 0, "created_at": at, "tags": [], "content": "{}" });
    assert_eq!(sign(&app, &Keys::generate().public_key().to_hex(), &admin, good.clone()).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(sign(&app, &admin, &admin, good.clone()).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(sign(&app, "not-a-key", &admin, good.clone()).await.status(), StatusCode::NOT_FOUND);

    // Nothing was signed, so nothing was logged.
    assert!(logs(&app).await.is_empty());
}

#[tokio::test]
async fn a_signer_cannot_and_a_retired_identity_signs_nothing() {
    let sales = Keys::generate();
    let app = common::spawn_with_identities(true, None, &[("Sales", sales.clone())]).await;
    let admin = app.admin.public_key().to_hex();
    let key = sales.public_key().to_hex();
    let good = json!({ "kind": 0, "created_at": now(), "tags": [], "content": "{}" });

    // A signer granted the identity still may not edit its profile.
    let signer = app.register(Role::Signer).public_key().to_hex();
    let path = format!("/api/bunker/identities/{key}/members/{signer}");
    assert_eq!(app.signed(Method::PUT, &path, &admin).send().await.unwrap().status(), StatusCode::OK);
    let res = sign(&app, &key, &signer, good.clone()).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"], "forbidden");

    // Retired: not found, and its key is gone.
    let retired = app.signed(Method::DELETE, &format!("/api/bunker/identities/{key}"), &admin).send().await.unwrap();
    assert_eq!(retired.status(), StatusCode::OK);
    assert_eq!(sign(&app, &key, &admin, good).await.status(), StatusCode::NOT_FOUND);
    assert!(logs(&app).await.is_empty());
}
