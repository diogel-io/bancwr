// The signing log and audit trail over HTTP (#96): paging, export and retention, with
// BANCWR_PROXY_SECRET set, so only administrators reach them.
#[path = "../common/mod.rs"]
mod common;

use bunker::audit::action;
use bunker::registry::Role;
use chrono::{Duration, TimeZone, Utc};
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const CLIENT: &str = "cc00000000000000000000000000000000000000000000000000000000000003";

async fn json_of(res: reqwest::Response) -> Value {
    res.json().await.unwrap()
}

#[tokio::test]
async fn the_log_pages_with_limit_and_before_until_it_runs_out() {
    let app = common::spawn(true).await;
    let base = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
    for i in 0..7 {
        app.db.log_signing_event(&format!("e{}", i), CLIENT, None, None, 1, base + Duration::seconds(i)).unwrap();
    }

    let mut seen = Vec::new();
    let mut path = "/api/bunker/logs?limit=3".to_string();
    loop {
        let res = app.get(&path).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let page: Vec<Value> = res.json().await.unwrap();
        seen.extend(page.iter().map(|l| l["event_id"].as_str().unwrap().to_string()));
        if page.len() < 3 {
            break;
        }
        // As the console builds it: the last row's timestamp and id.
        let last = page.last().unwrap();
        let cursor = format!("{}|{}", last["timestamp"].as_str().unwrap(), last["id"].as_str().unwrap());
        path = format!("/api/bunker/logs?limit=3&before={}", urlencode(&cursor));
    }
    assert_eq!(seen, ["e6", "e5", "e4", "e3", "e2", "e1", "e0"]);

    // No parameters: the newest 100, an array, as the dashboard has always read it.
    let all: Vec<Value> = json_of(app.get("/api/bunker/logs").send().await.unwrap()).await.as_array().unwrap().clone();
    assert_eq!(all.len(), 7);
}

fn urlencode(value: &str) -> String {
    value.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
        _ => format!("%{:02X}", b),
    }).collect()
}

#[tokio::test]
async fn a_bad_limit_or_cursor_is_400() {
    let app = common::spawn(true).await;
    for (path, code) in [
        ("/api/bunker/logs?limit=0", "invalid_limit"),
        ("/api/bunker/logs?limit=501", "invalid_limit"),
        ("/api/bunker/logs?before=nonsense", "invalid_cursor"),
    ] {
        let res = app.get(path).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(json_of(res).await["error"], code, "{path}");
    }
}

#[tokio::test]
async fn an_administrator_downloads_a_csv_of_signatures_and_audit_events_in_the_range() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    let signer = app.register(Role::Signer).public_key().to_hex();
    let day = Utc::now().date_naive();
    app.db.log_signing_event("in-range", CLIENT, Some(&signer), Some("conn-1"), 7, Utc::now()).unwrap();
    app.db.log_signing_event("too-old", CLIENT, None, None, 1, Utc::now() - Duration::days(3)).unwrap();

    let res = app.get(&format!("/api/bunker/logs/export?format=csv&from={day}&to={day}")).send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/csv; charset=utf-8");
    assert_eq!(res.headers()["content-disposition"], format!("attachment; filename=\"bancwr-log-{day}-{day}.csv\""));
    assert_eq!(res.headers()["cache-control"], "no-store");
    let body = res.text().await.unwrap();
    let lines: Vec<&str> = body.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines[0],
        "time,source,action,event_kind,event_id,member_pubkey,member_name,connection_id,token_id,client_pubkey,actor_pubkey,detail"
    );
    let signed = lines.iter().find(|l| l.contains("in-range")).expect("the signature in range");
    assert!(signed.contains(&format!(",signature,signed,7,in-range,{signer},Member,conn-1,,{CLIENT},,")), "{signed}");
    assert!(!body.contains("too-old"));
    // The member was added today, and the export itself is recorded before its rows are read.
    assert!(lines.iter().any(|l| l.contains(",audit,member_added,") && l.contains(&signer)));
    let exported = lines.iter().find(|l| l.contains(",audit,logs_exported,")).expect("the export recorded");
    assert!(exported.contains(&admin) && exported.ends_with(&format!("format=csv;from={day};to={day}")), "{exported}");
}

#[tokio::test]
async fn jsonl_has_one_object_per_row_with_the_same_fields() {
    let app = common::spawn(true).await;
    let day = Utc::now().date_naive();
    app.db.log_signing_event("one", CLIENT, None, None, 1, Utc::now()).unwrap();

    let res = app.get(&format!("/api/bunker/logs/export?format=jsonl&from={day}&to={day}")).send().await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "application/x-ndjson");
    let rows: Vec<Value> = res.text().await.unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let signed = rows.iter().find(|r| r["event_id"] == "one").unwrap();
    let keys: Vec<&str> = signed.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys.len(), bunker::logs_api::EXPORT_COLUMNS.len());
    for column in bunker::logs_api::EXPORT_COLUMNS {
        assert!(signed.get(column).is_some(), "{column}");
    }
    assert_eq!((signed["source"].as_str(), signed["action"].as_str(), signed["event_kind"].as_u64()), (Some("signature"), Some("signed"), Some(1)));
    assert!(rows.iter().any(|r| r["action"] == action::LOGS_EXPORTED));
}

#[tokio::test]
async fn an_export_larger_than_one_batch_is_complete_and_in_order() {
    let app = common::spawn(true).await;
    let day = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let total = bunker::logs_api::EXPORT_BATCH * 2 + 17;
    for i in 0..total {
        app.db.log_signing_event(&format!("e{:05}", i), CLIENT, None, None, 1, day + Duration::seconds(i as i64)).unwrap();
    }

    let body = app.get("/api/bunker/logs/export?format=csv&from=2026-06-01&to=2026-06-01").send().await.unwrap().text().await.unwrap();

    let ids: Vec<&str> = body.split("\r\n").skip(1).filter(|l| !l.is_empty()).map(|l| l.split(',').nth(4).unwrap()).collect();
    assert_eq!(ids.len(), total);
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "oldest first, none twice");
}

#[tokio::test]
async fn a_bad_format_or_range_is_400() {
    let app = common::spawn(true).await;
    for (query, code) in [
        ("format=xlsx&from=2026-06-01&to=2026-06-01", "invalid_format"),
        ("format=csv", "invalid_range"),
        ("format=csv&from=2026-06-01", "invalid_range"),
        ("format=csv&from=yesterday&to=2026-06-01", "invalid_range"),
        ("format=csv&from=2026-06-02&to=2026-06-01", "invalid_range"),
    ] {
        let res = app.get(&format!("/api/bunker/logs/export?{query}")).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(json_of(res).await["error"], code, "{query}");
    }
    assert_eq!(app.db.audit_events().unwrap().iter().filter(|e| e.action == action::LOGS_EXPORTED).count(), 0, "nothing exported");
}

#[tokio::test]
async fn signers_and_viewers_cannot_export_or_change_retention() {
    let app = common::spawn(true).await;
    for role in [Role::Signer, Role::Viewer] {
        let member = app.register(role).public_key().to_hex();
        for (method, path) in [
            (Method::GET, "/api/bunker/logs/export?format=csv&from=2026-06-01&to=2026-06-01"),
            (Method::GET, "/api/bunker/logs/retention"),
            (Method::PUT, "/api/bunker/logs/retention"),
            (Method::GET, "/api/bunker/logs?limit=500"),
        ] {
            let res = app.signed(method.clone(), path, &member).json(&json!({ "max_age_days": null, "max_rows": null })).send().await.unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{role} {method} {path}");
        }
    }
    assert_eq!(app.db.audit_events().unwrap().iter().filter(|e| e.action == action::LOGS_EXPORTED).count(), 0);
}

#[tokio::test]
async fn retention_is_read_set_applied_at_once_and_recorded() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    app.db.log_signing_event("old", CLIENT, None, None, 1, Utc::now() - Duration::days(100)).unwrap();
    app.db.log_signing_event("new", CLIENT, None, None, 1, Utc::now()).unwrap();

    let initial = json_of(app.get("/api/bunker/logs/retention").send().await.unwrap()).await;
    assert_eq!(initial, json!({ "max_age_days": null, "max_rows": null, "min_age_days": 30, "min_rows": 10000 }));

    let res = app.request(Method::PUT, "/api/bunker/logs/retention").json(&json!({ "max_age_days": 90, "max_rows": null })).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_of(res).await["max_age_days"], 90);

    assert_eq!(app.db.get_recent_logs(10).unwrap().iter().map(|l| l.event_id.as_str()).collect::<Vec<_>>(), ["new"], "pruned at once");
    let events = app.db.audit_events().unwrap();
    let changed = events.iter().find(|e| e.action == action::RETENTION_CHANGED).unwrap();
    assert_eq!(changed.actor_pubkey.as_deref(), Some(admin.as_str()));
    assert!(events.iter().any(|e| e.action == action::LOGS_PRUNED && e.detail.as_deref() == Some("signatures=1;audit_events=0")));
}

#[tokio::test]
async fn retention_under_the_floors_is_400_and_changes_nothing() {
    let app = common::spawn(true).await;
    for (body, code) in [
        (json!({ "max_age_days": 7, "max_rows": null }), "invalid_max_age"),
        (json!({ "max_age_days": null, "max_rows": 100 }), "invalid_max_rows"),
    ] {
        let res = app.request(Method::PUT, "/api/bunker/logs/retention").json(&body).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_of(res).await["error"], code);
    }
    assert_eq!(app.db.log_retention().unwrap(), bunker::audit::Retention::default());
    assert!(!app.db.audit_events().unwrap().iter().any(|e| e.action == action::RETENTION_CHANGED));
}

#[tokio::test]
async fn changes_made_through_the_api_name_the_administrator() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    let new_member = Keys::generate().public_key().to_hex();

    let added = app.request(Method::POST, "/api/bunker/team").json(&json!({ "name": "New", "pubkey": new_member, "role": "signer" })).send().await.unwrap();
    assert_eq!(added.status(), StatusCode::OK);
    let id = app.db.find_member_by_pubkey(&new_member).unwrap().unwrap().id;
    let removed = app.request(Method::DELETE, &format!("/api/bunker/team/{id}")).send().await.unwrap();
    assert_eq!(removed.status(), StatusCode::OK);

    let events = app.db.audit_events().unwrap();
    for wanted in [action::MEMBER_ADDED, action::MEMBER_REMOVED] {
        let event = events.iter().rev().find(|e| e.action == wanted && e.member_pubkey.as_deref() == Some(new_member.as_str())).unwrap();
        assert_eq!(event.actor_pubkey.as_deref(), Some(admin.as_str()), "{wanted}");
        assert_eq!(event.member_name.as_deref(), Some("New"), "{wanted}");
    }
}
