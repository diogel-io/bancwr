// Who may call /api/bunker/* (#25), with BANCWR_PROXY_SECRET set. Every route is listed, not
// sampled, so a route added without a guard fails here.
#[path = "../common/mod.rs"]
mod common;

use bunker::registry::Role;
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::Value;

const ADMINISTRATOR_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/bunker/logs"),
    ("GET", "/api/bunker/metrics"),
    ("GET", "/api/bunker/config"),
    ("GET", "/api/bunker/team"),
    ("POST", "/api/bunker/team"),
    ("DELETE", "/api/bunker/team/00000000-0000-4000-8000-000000000000"),
];
const STATUS: (&str, &str) = ("GET", "/api/bunker/status");

fn lookup_route(app: &common::TestApp) -> (&'static str, String) {
    ("GET", format!("/api/bunker/team/by-pubkey/{}", app.admin.public_key().to_hex()))
}

fn all_routes(app: &common::TestApp) -> Vec<(Method, String)> {
    let mut routes: Vec<(&str, String)> = ADMINISTRATOR_ROUTES.iter().map(|(m, p)| (*m, p.to_string())).collect();
    routes.push((STATUS.0, STATUS.1.to_string()));
    routes.push(lookup_route(app));
    routes.into_iter().map(|(m, p)| (m.parse().unwrap(), p)).collect()
}

async fn body(res: reqwest::Response) -> Value {
    res.json().await.unwrap_or(Value::Null)
}

/// A valid POST body, so an allowed POST is not refused for its payload.
fn member_body() -> Value {
    serde_json::json!({ "name": "New", "pubkey": Keys::generate().public_key().to_bech32().unwrap(), "role": "user" })
}

#[tokio::test]
async fn every_api_route_is_401_without_a_signature() {
    let app = common::spawn(true).await;
    for (method, path) in all_routes(&app) {
        let res = app.unsigned(method.clone(), &path).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{} {}", method, path);
        let body = body(res).await;
        assert_eq!(body["error"], "not_authenticated", "{} {}", method, path);
        assert_eq!(body["reason"], "missing");
    }
}

#[tokio::test]
async fn a_signature_under_another_secret_or_for_another_request_is_401() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    let now = chrono::Utc::now().timestamp();

    let wrong_secret = app.signed_with(Method::GET, "/api/bunker/team", &admin, "not-the-secret-not-the-secret-000", now);
    assert_eq!(body(wrong_secret.send().await.unwrap()).await["reason"], "bad_signature");

    // Signed for GET /api/bunker/status, sent to /api/bunker/team.
    let signature = bunker::proxy_auth::proxy_signature(common::SECRET.as_bytes(), now, "GET", "/api/bunker/status", &admin);
    let moved = app
        .unsigned(Method::GET, "/api/bunker/team")
        .header(bunker::proxy_auth::IDENTITY_HEADER, &admin)
        .header(bunker::proxy_auth::TIMESTAMP_HEADER, now.to_string())
        .header(bunker::proxy_auth::SIGNATURE_HEADER, signature)
        .send()
        .await
        .unwrap();
    assert_eq!(moved.status(), StatusCode::UNAUTHORIZED);

    let stale = app.signed_with(Method::GET, "/api/bunker/team", &admin, common::SECRET, now - 31);
    assert_eq!(body(stale.send().await.unwrap()).await["reason"], "stale");

    let npub_identity = app.signed(Method::GET, "/api/bunker/team", &app.admin.public_key().to_bech32().unwrap());
    assert_eq!(body(npub_identity.send().await.unwrap()).await["reason"], "malformed");
}

#[tokio::test]
async fn an_unregistered_key_is_403_not_registered_with_its_npub() {
    let app = common::spawn(true).await;
    let stranger = Keys::generate();
    for (method, path) in all_routes(&app) {
        let res = app.signed(method.clone(), &path, &stranger.public_key().to_hex()).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{} {}", method, path);
        let body = body(res).await;
        assert_eq!(body["error"], "not_registered", "{} {}", method, path);
        assert_eq!(body["npub"], stranger.public_key().to_bech32().unwrap());
    }
}

#[tokio::test]
async fn user_and_signer_are_refused_administration_but_see_health() {
    let app = common::spawn(true).await;
    for role in [Role::User, Role::Signer] {
        let member = app.register(role).public_key().to_hex();
        for (method, path) in ADMINISTRATOR_ROUTES.iter().map(|(m, p)| (m.parse::<Method>().unwrap(), p.to_string())).chain([lookup_route(&app)].map(|(m, p)| (m.parse().unwrap(), p))) {
            let res = app.signed(method.clone(), &path, &member).send().await.unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{} on {} {}", role, method, path);
            assert_eq!(body(res).await["error"], "forbidden");
        }
        let res = app.signed(Method::GET, STATUS.1, &member).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{} sees bunker health", role);
    }
}

#[tokio::test]
async fn an_administrator_reaches_every_route() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    for (method, path) in all_routes(&app) {
        let mut request = app.signed(method.clone(), &path, &admin);
        if method == Method::POST {
            request = request.json(&member_body());
        }
        let status = request.send().await.unwrap().status();
        // DELETE of an unknown id is 404 from the handler: the guard let it through.
        let expected = if method == Method::DELETE { StatusCode::NOT_FOUND } else { StatusCode::OK };
        assert_eq!(status, expected, "{} {}", method, path);
    }
}

#[tokio::test]
async fn the_service_identity_may_only_read_status_and_look_a_key_up() {
    let app = common::spawn(true).await;
    let service = bunker::proxy_auth::SERVICE_IDENTITY;

    let status = app.signed(Method::GET, STATUS.1, service).send().await.unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let (_, lookup) = lookup_route(&app);
    let found = app.signed(Method::GET, &lookup, service).send().await.unwrap();
    assert_eq!(found.status(), StatusCode::OK);
    assert_eq!(body(found).await["role"], "administrator");

    for (method, path) in ADMINISTRATOR_ROUTES {
        let res = app.signed(method.parse().unwrap(), path, service).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "service on {} {}", method, path);
    }
}

#[tokio::test]
async fn a_removed_member_loses_access_on_the_next_request() {
    let app = common::spawn(true).await;
    let second_admin = app.register(Role::Administrator);
    let key = second_admin.public_key().to_hex();
    assert_eq!(app.signed(Method::GET, "/api/bunker/team", &key).send().await.unwrap().status(), StatusCode::OK);

    let member = app.db.find_member_by_pubkey(&key).unwrap().unwrap();
    let removed = app.request(Method::DELETE, &format!("/api/bunker/team/{}", member.id)).send().await.unwrap();
    assert_eq!(removed.status(), StatusCode::OK);

    // No cached role: the very next request is refused.
    let res = app.signed(Method::GET, "/api/bunker/team", &key).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(body(res).await["error"], "not_registered");
}

#[tokio::test]
async fn health_and_unknown_routes_need_no_credentials() {
    let app = common::spawn(true).await;
    assert_eq!(app.unsigned(Method::GET, "/health").send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(app.unsigned(Method::POST, "/sign").send().await.unwrap().status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn there_are_no_cors_headers() {
    // Browsers only call the frontend's server; the bunker answers no cross-origin request.
    let app = common::spawn(true).await;
    let res = app
        .unsigned(Method::OPTIONS, "/api/bunker/status")
        .header("Origin", "https://evil.example")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();
    assert!(res.headers().get("access-control-allow-origin").is_none());
}
