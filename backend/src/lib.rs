pub mod api;
pub mod config_store;
pub mod db;
pub mod export;
pub mod models;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use config_store::ConfigStore;
use std::sync::Once;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Start the Axum server on the given port (or default from env/3000). Returns the bound address.
pub async fn start_server(port: Option<u16>) -> Result<SocketAddr> {
    dotenv::dotenv().ok();

    let config_store = Arc::new(
        ConfigStore::ensure_default_path().context("Failed to initialize config store")?,
    );

    let app_state = api::AppState { config_store };
    let app = api::create_router(app_state);

    let port = port
        .or_else(|| std::env::var("SERVER_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(3000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;
    let bound = listener.local_addr().context("Unable to read bound address")?;

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!(error = ?err, "Server exited with error");
        }
    });

    Ok(bound)
}

/// Initialize tracing with env filter defaults. Safe to call multiple times.
pub fn init_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "dm8_export_backend=debug,tower_http=debug".into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .try_init();
    });
}
