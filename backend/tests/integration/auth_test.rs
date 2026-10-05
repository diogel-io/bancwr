// Who may call /api/bunker/* (#25, #77), with BANCWR_PROXY_SECRET set. Every route is listed, not
// sampled, so a route added without a guard fails here.
#[path = "../common/mod.rs"]
mod common;

use bunker::registry::Role;
use nostr::prelude::*;
use reqwest::{Method, StatusCode};
use serde_json::Value;

/// Who may call each route (#77): `A`dministrator, `S`igner, `V`iewer, and the service identity.
/// Every guarded route is listed, so a route added without a guard, or in the wrong group, fails.
struct Route {
    method: &'static str,
    path: &'static str,
    roles: &'static [Role],
    service: bool,
}

const A: Role = Role::Administrator;
const S: Role = Role::Signer;
const V: Role = Role::Viewer;
/// Replaced with the administrator's own key, so the lookup finds someone.
const LOOKUP: &str = "/api/bunker/team/by-pubkey/{admin}";

const ROUTES: &[Route] = &[
    Route { method: "GET", path: "/api/bunker/status", roles: &[A, S, V], service: true },
    Route { method: "GET", path: "/api/bunker/logs", roles: &[A], service: false },
    Route { method: "GET", path: "/api/bunker/logs/mine", roles: &[A, S], service: false },
    Route { method: "GET", path: "/api/bunker/metrics", roles: &[A], service: false },
    Route { method: "GET", path: "/api/bunker/config", roles: &[A], service: false },
    Route { method: "GET", path: "/api/bunker/relays", roles: &[A], service: false },
    Route { method: "PUT", path: "/api/bunker/relays", roles: &[A], service: false },
    Route { method: "GET", path: "/api/bunker/team", roles: &[A, V], service: false },
    Route { method: "POST", path: "/api/bunker/team", roles: &[A], service: false },
    Route { method: "DELETE", path: "/api/bunker/team/00000000-0000-4000-8000-000000000000", roles: &[A], service: false },
    Route { method: "GET", path: LOOKUP, roles: &[A, V], service: true },
    Route { method: "GET", path: "/api/bunker/connections/tokens", roles: &[A], service: false },
    Route { method: "POST", path: "/api/bunker/connections/tokens", roles: &[A], service: false },
    Route { method: "DELETE", path: "/api/bunker/connections/tokens/no-such-token", roles: &[A], service: false },
    Route { method: "GET", path: "/api/bunker/connections", roles: &[A, S], service: false },
    Route { method: "DELETE", path: "/api/bunker/connections/no-such-connection", roles: &[A, S], service: false },
];

fn path_of(app: &common::TestApp, route: &Route) -> String {
    route.path.replace("{admin}", &app.admin.public_key().to_hex())
}

fn all_routes(app: &common::TestApp) -> Vec<(Method, String)> {
    ROUTES.iter().map(|r| (r.method.parse().unwrap(), path_of(app, r))).collect()
}

/// A signed request for `route` as `identity`, with a valid body, so an allowed write is not
/// refused for its payload before the guard's decision can be seen.
fn request_for(app: &common::TestApp, route: &Route, identity: &str) -> reqwest::RequestBuilder {
    let request = app.signed(route.method.parse().unwrap(), &path_of(app, route), identity);
    match (route.method, route.path) {
        ("POST", "/api/bunker/team") => request.json(&member_body()),
        ("PUT", "/api/bunker/relays") => request.json(&serde_json::json!({ "relays": [] })),
        ("POST", "/api/bunker/connections/tokens") => {
            request.json(&serde_json::json!({ "label": "x", "kinds": [1] }))
        }
        _ => request,
    }
}

async fn body(res: reqwest::Response) -> Value {
    res.json().await.unwrap_or(Value::Null)
}

/// A valid POST body, so an allowed POST is not refused for its payload.
fn member_body() -> Value {
    serde_json::json!({ "name": "New", "pubkey": Keys::generate().public_key().to_bech32().unwrap(), "role": "signer" })
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

/// The access matrix (#77), every route for every role: an allowed caller gets past the guard
/// (whatever the handler then answers), a refused one gets 403 `forbidden`.
#[tokio::test]
async fn each_role_reaches_exactly_its_routes() {
    let app = common::spawn(true).await;
    for role in [A, S, V] {
        let member = if role == A { app.admin.public_key().to_hex() } else { app.register(role).public_key().to_hex() };
        for route in ROUTES {
            let res = request_for(&app, route, &member).send().await.unwrap();
            let status = res.status();
            if route.roles.contains(&role) {
                assert!(
                    status != StatusCode::FORBIDDEN && status != StatusCode::UNAUTHORIZED,
                    "{} should reach {} {}, got {}",
                    role, route.method, route.path, status
                );
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{} on {} {}", role, route.method, route.path);
                assert_eq!(body(res).await["error"], "forbidden", "{} on {} {}", role, route.method, route.path);
            }
        }
    }
}

#[tokio::test]
async fn an_administrator_reaches_every_route() {
    let app = common::spawn(true).await;
    let admin = app.admin.public_key().to_hex();
    for route in ROUTES {
        let status = request_for(&app, route, &admin).send().await.unwrap().status();
        // DELETE of an unknown id is 404 from the handler, and a token with NIP-46 off is 409:
        // either way the guard let it through.
        let expected = match route.method {
            "DELETE" => StatusCode::NOT_FOUND,
            "POST" if route.path.ends_with("/tokens") => StatusCode::CONFLICT,
            _ => StatusCode::OK,
        };
        assert_eq!(status, expected, "{} {}", route.method, route.path);
    }
}

#[tokio::test]
async fn the_service_identity_may_only_read_status_and_look_a_key_up() {
    let app = common::spawn(true).await;
    let service = bunker::proxy_auth::SERVICE_IDENTITY;
    for route in ROUTES {
        let res = request_for(&app, route, service).send().await.unwrap();
        if route.service {
            assert_eq!(res.status(), StatusCode::OK, "service on {} {}", route.method, route.path);
        } else {
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "service on {} {}", route.method, route.path);
        }
    }
    let lookup = LOOKUP.replace("{admin}", &app.admin.public_key().to_hex());
    let found = app.signed(Method::GET, &lookup, service).send().await.unwrap();
    assert_eq!(body(found).await["role"], "administrator");
}

#[tokio::test]
async fn a_viewer_reads_the_team_but_cannot_change_it() {
    let app = common::spawn(true).await;
    let viewer = app.register(V).public_key().to_hex();

    let team = app.signed(Method::GET, "/api/bunker/team", &viewer).send().await.unwrap();
    assert_eq!(team.status(), StatusCode::OK);
    assert_eq!(body(team).await.as_array().unwrap().len(), 2, "the administrator and the viewer");

    let added = app.signed(Method::POST, "/api/bunker/team", &viewer).json(&member_body()).send().await.unwrap();
    assert_eq!(added.status(), StatusCode::FORBIDDEN);
    let admin_id = app.db.find_member_by_pubkey(&app.admin.public_key().to_hex()).unwrap().unwrap().id;
    let removed = app.signed(Method::DELETE, &format!("/api/bunker/team/{}", admin_id), &viewer).send().await.unwrap();
    assert_eq!(removed.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.db.get_team_members().unwrap().len(), 2, "nothing changed");
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
