use bunker::config::Config;
use bunker::db::{Database, SeedOutcome};
use bunker::relay::RelayClient;
use bunker::server::{create_router, shutdown_signal};
use bunker::state::AppState;
use bunker::signer::Signer;
use std::net::SocketAddr;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

    let config = Config::load()?;
    info!("Bancwr Diogel {} starting...", config.version);
    let db = Database::new(&config.db_path)?;

    // Earlier versions stored a key submitted on the Config page in SQLite, in plain text, and
    // never used it (#42). Remove it on every start.
    if db.purge_stored_key_config()? {
        warn!(
            "Deleted an nsec stored in the database by an earlier version's Config page. It was never \
             used: the signing key comes from BUNKER_NSEC_FILE or BUNKER_NSEC. It was stored in plain \
             text, so if this database was ever copied or backed up, rotate that key."
        );
    }

    // First-administrator bootstrap (#24). Nobody can administer the bunker until someone holds
    // the administrator role, and once #25 enforces roles only an administrator can add members.
    match &config.admin_pubkey {
        Some(pubkey) => match db.seed_administrator(pubkey)? {
            SeedOutcome::AdministratorExists => {}
            SeedOutcome::Added => info!("Registered BANCWR_ADMIN_PUBKEY as the first administrator"),
            SeedOutcome::Promoted => info!("Promoted BANCWR_ADMIN_PUBKEY to administrator: there was none"),
        },
        None if db.administrator_count()? == 0 => warn!(
            "No administrator is registered, so nobody can administer this bunker. Set \
             BANCWR_ADMIN_PUBKEY to the first administrator's npub and restart."
        ),
        None => {}
    }

    if config.proxy_secret.is_none() {
        warn!(
            "BANCWR_PROXY_SECRET is not set, so /api/bunker/* is unauthenticated: anyone who can \
             reach this port can read and change the team. Set it (and NUXT_PROXY_SECRET on the \
             frontend) to the same value; sign-in (#11) will require it."
        );
    }

    let signer = Signer::new(config.secret_key.clone());
    info!("Nsec loaded. Public key: {}", signer.public_key_bech32());

    let state = AppState::new(signer.clone(), db, config.clone());

    let port = config.port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("HTTP API listening on http://{}", addr);

    let http_state = state.clone();
    let http_task = tokio::spawn(async move {
        let app = create_router(http_state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| anyhow::anyhow!("HTTP server error: {}", e))
    });

    // Start NIP-46 relay client (if enabled)
    let relay_task = if config.nip46_enabled {
        let relay_state = state.clone();
        Some(tokio::spawn(async move {
            info!("Starting NIP-46 relay client...");
            let client = RelayClient::new(
                config.relay_urls,
                relay_state,
            ).await?;
            client.run().await
        }))
    } else {
        None
    };

    // Wait for shutdown signal or tasks
    tokio::select! {
        result = http_task => {
            if let Ok(Err(e)) = result {
                error!("HTTP server error: {}", e);
            }
        }
        result = async {
            if let Some(task) = relay_task {
                task.await
            } else {
                std::future::pending().await
            }
        } => {
            if let Ok(Err(e)) = result {
                error!("Relay client error: {}", e);
            }
        }
        _ = shutdown_signal() => {
            info!("Shutdown signal received");
        }
    }

    Ok(())
}
