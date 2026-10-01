// `bunker healthcheck` (#16): the distroless image's health probe.
use axum::{http::StatusCode, routing::get, Router};
use bunker::healthcheck::check;
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
        secret_key: keys.secret_key().clone(),
        port: 0,
        db_path: ":memory:".to_string(),
        relay_urls: vec![],
        nip46_enabled: false,
        nsec_file: None,
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
