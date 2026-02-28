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

    // On Windows, ensure the bundled DM8 ODBC driver is registered in the registry
    // so the ODBC Driver Manager can find it by name in connection strings.
    #[cfg(windows)]
    {
        let driver_dll = std::env::var("DM8_DRIVER_PATH").unwrap_or_default();
        if driver_dll.trim().is_empty() {
            tracing::warn!("DM8_DRIVER_PATH not set; ODBC driver registration skipped");
        } else {
            if let Err(e) = db::odbc_register::ensure_dm8_driver_registered(driver_dll.trim()) {
                tracing::warn!("DM8 ODBC driver registration failed (may need admin): {}", e);
            }
        }
    }

    let config_store =
        Arc::new(ConfigStore::ensure_default_path().context("Failed to initialize config store")?);

    let app_state = api::AppState { config_store };
    let app = api::create_router(app_state);

    let port = port
        .or_else(|| {
            std::env::var("SERVER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
        })
        .unwrap_or(3000);

    let bind_ip: [u8; 4] = std::env::var("BIND_ADDRESS")
        .ok()
        .and_then(|s| {
            let parts: Vec<u8> = s.split('.').filter_map(|p| p.parse().ok()).collect();
            if parts.len() == 4 {
                Some([parts[0], parts[1], parts[2], parts[3]])
            } else {
                None
            }
        })
        .unwrap_or([127, 0, 0, 1]);
    let addr = SocketAddr::from((bind_ip, port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;
    let bound = listener
        .local_addr()
        .context("Unable to read bound address")?;

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
