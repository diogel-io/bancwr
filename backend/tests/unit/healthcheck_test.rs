// `bunker healthcheck` (#16): the distroless image's health probe.
use axum::{http::StatusCode, routing::get, Router};
use bunker::healthcheck::{check, locked_reason};
use tokio::net::TcpListener;

async fn serve(app: Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

/// The probe is blocking, as it is in the container; run it off the async runtime.
async fn probe(port: u16) -> Result<(), String> {
    tokio::task::spawn_blocking(move || check(port)).await.unwrap()
}

#[tokio::test]
async fn healthy_when_health_answers_200() {
    let port = serve(Router::new().route("/health", get(|| async { "{\"status\":\"ok\"}" }))).await;
    assert_eq!(probe(port).await, Ok(()));
}

#[tokio::test]
async fn unhealthy_when_health_answers_an_error() {
    let port = serve(Router::new().route("/health", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))).await;
    let result = probe(port).await;
    assert!(result.unwrap_err().contains("503"));
}

#[tokio::test]
async fn unhealthy_when_nothing_listens() {
    // Bind and drop, so the port is very likely free.
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let result = probe(port).await;
    assert!(result.unwrap_err().contains("cannot connect"));
}

#[tokio::test]
async fn the_real_health_route_passes() {
    // Against the bunker's own router, not a stand-in.
    let keys = nostr::Keys::generate();
    let config = bunker::config::Config {
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        version: "0.0.0".to_string(),
        admin_pubkey: None,
        proxy_secret: Some("s".repeat(32)),
    };
    let state = bunker::state::AppState::new(
        bunker::signer::Signer::new(keys.secret_key().clone()),
        bunker::db::Database::new(":memory:").unwrap(),
        config,
    );
    let port = serve(bunker::server::create_router(state)).await;
    assert_eq!(probe(port).await, Ok(()));
}

#[tokio::test]
async fn a_locked_bunker_s_reason_is_printed() {
    // #94: a locked bunker answers 503 with why, and the probe says it.
    let body = r#"{"status":"locked","reason":"The root credential does not decrypt the bunker's key."}"#;
    let app = Router::new().route("/health", get(move || async move {
        (StatusCode::SERVICE_UNAVAILABLE, [("content-type", "application/json")], body)
    }));
    let error = probe(serve(app).await).await.unwrap_err();
    assert!(error.contains("503"), "{}", error);
    assert!(error.ends_with("locked: The root credential does not decrypt the bunker's key."), "{}", error);
}

#[test]
fn only_a_json_reason_is_taken() {
    assert_eq!(locked_reason("HTTP/1.1 503 x\r\n\r\n{\"reason\":\"why\"}"), Some("why".to_string()));
    assert_eq!(locked_reason("HTTP/1.1 503 x\r\n\r\nnot json"), None);
    assert_eq!(locked_reason("HTTP/1.1 503 x\r\n\r\n{\"status\":\"locked\"}"), None);
    assert_eq!(locked_reason("HTTP/1.1 503 x"), None);
}

#[tokio::test]
async fn the_real_locked_router_fails_the_probe_with_its_reason() {
    let db = std::sync::Arc::new(bunker::db::Database::new(":memory:").unwrap());
    let state = bunker::server::LockedState {
        reason: bunker::key_store::PLAIN_KEY_STILL_SET.to_string(),
        pubkey: String::new(),
        version: "0.0.0".to_string(),
        nip46_enabled: false,
        db,
    };
    let port = serve(bunker::server::create_locked_router(state, Some("s".repeat(32)))).await;
    let error = probe(port).await.unwrap_err();
    assert!(error.ends_with(&format!("locked: {}", bunker::key_store::PLAIN_KEY_STILL_SET)), "{}", error);
}
