use bunker::bunker_relays::{self, EffectiveRelays, RelaySource};
use bunker::config::Config;
use bunker::db::{Database, SeedOutcome};
use bunker::relay::RelayClient;
use bunker::server::{create_router, shutdown_signal};
use bunker::state::AppState;
use bunker::signer::Signer;
use std::net::SocketAddr;
use tracing::{error, info, warn};

fn main() -> anyhow::Result<()> {
    // `bunker healthcheck`: the container's probe (#16). Handled before anything else, so it needs
    // neither the signing key nor a runtime.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        match bunker::healthcheck::check(bunker::healthcheck::port_from_env()) {
            Ok(()) => std::process::exit(0),
            Err(reason) => {
                eprintln!("{}", reason);
                std::process::exit(1);
            }
        }
    }
    serve()
}

#[tokio::main]
async fn serve() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

    let config = Config::load()?;
    // Before touching the database: sign-in (#11) signs every proxied request, so the API is never
    // left unauthenticated.
    config.require_proxy_secret()?;
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
             BANCWR_ADMIN_PUBKEY to the first administrator's npub and restart. It cannot be the \
             bunker's own key (BUNKER_NSEC): the bunker signs for others, so that key never signs in."
        ),
        None => {}
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

    // Start NIP-46 relay client (if enabled), on the relays in force (#78): NIP46_RELAYS when set,
    // otherwise the list saved in the console. It starts with none too, so the first list an
    // administrator saves takes effect without a restart.
    let relay_task = if config.nip46_enabled {
        let relay_state = state.clone();
        Some(tokio::spawn(async move {
            info!("Starting NIP-46 relay client...");
            let client = {
                // Held until the client's handle is in the state, so a list saved meanwhile is not
                // stored without being applied.
                let _update = relay_state.relays_update.lock().await;
                let relays = bunker_relays::effective_relays(&relay_state).await?;
                log_relay_source(&relays, &relay_state.db);
                RelayClient::new(relays.urls, relay_state.clone()).await?
            };
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

/// Says which relays the bunker starts on, and where they come from (#78).
fn log_relay_source(relays: &EffectiveRelays, db: &Database) {
    match relays.source {
        RelaySource::Environment => {
            info!("NIP-46 relays from NIP46_RELAYS: {}", relays.urls.join(", "));
            match db.list_bunker_relays() {
                Ok(stored) if !stored.is_empty() => warn!(
                    "NIP46_RELAYS is set, so the {} relays saved in the console are ignored. Unset it \
                     and restart to use them.",
                    stored.len()
                ),
                Ok(_) => {}
                Err(e) => warn!("Could not read the relays saved in the console: {}", e),
            }
        }
        RelaySource::Console if relays.urls.is_empty() => warn!(
            "NIP-46 is on but no relays are configured, so no app can reach the bunker yet. An \
             administrator sets them under Config, Bunker relays, or set NIP46_RELAYS."
        ),
        RelaySource::Console => info!("NIP-46 relays from the console: {}", relays.urls.join(", ")),
    }
}
