pub mod api;
pub mod config_store;
pub mod db;
pub mod dialect;
pub mod domain;
pub mod export;
pub mod models;
pub mod source;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use config_store::ConfigStore;
use std::sync::Once;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(windows)]
fn canonicalize_dll_path(p: &std::path::Path) -> Option<String> {
    // std::fs::canonicalize on Windows returns \\?\ extended paths.
    // ODBC Driver Manager does not handle the \\?\ prefix, so strip it.
    p.canonicalize().ok().map(|abs| {
        let s = abs.display().to_string();
        s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string()
    })
}

#[cfg(windows)]
fn resolve_bundled_driver_dll(env_var: &str, candidates: &[&str]) -> Option<String> {
    if let Ok(raw) = std::env::var(env_var) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let p = std::path::Path::new(trimmed);
            if p.exists() {
                tracing::debug!("ODBC driver path from {}: {}", env_var, trimmed);
                // Canonicalize to absolute path — ODBC Driver Manager requires absolute DLL paths
                return Some(canonicalize_dll_path(p).unwrap_or_else(|| trimmed.to_string()));
            }
            tracing::warn!(
                "{} is set but file does not exist: {}; fallback to bundled path probing",
                env_var,
                trimmed
            );
        }
    }

    for candidate in candidates {
        let p = std::path::Path::new(candidate);
        if p.exists() {
            tracing::debug!("ODBC driver from bundled path: {}", candidate);
            // Canonicalize to absolute path — ODBC Driver Manager requires absolute DLL paths
            return Some(canonicalize_dll_path(p).unwrap_or_else(|| p.display().to_string()));
        }
    }

    None
}

#[cfg(windows)]
fn register_bundled_driver(driver_name: &str, env_var: &str, candidates: &[&str], required: bool) {
    match resolve_bundled_driver_dll(env_var, candidates) {
        Some(driver_dll) => {
            // Add the DLL's directory to both PATH and via AddDllDirectory Win32 API.
            // AddDllDirectory is more reliable on modern Windows: the ODBC Driver Manager
            // may use LoadLibraryEx with LOAD_LIBRARY_SEARCH_* flags that ignore PATH but
            // do honour directories registered via AddDllDirectory.
            if let Some(dir) = std::path::Path::new(&driver_dll).parent() {
                let dir_str = dir.display().to_string();

                // PATH (legacy search path — kept for compatibility)
                let current_path = std::env::var("PATH").unwrap_or_default();
                if !current_path
                    .split(';')
                    .any(|p| p.eq_ignore_ascii_case(&dir_str))
                {
                    let new_path = format!("{};{}", dir_str, current_path);
                    std::env::set_var("PATH", &new_path);
                    tracing::debug!("Added ODBC driver directory to PATH: {}", dir_str);
                }

                // AddDllDirectory (modern search path — works even with restricted flags)
                #[link(name = "kernel32")]
                extern "system" {
                    fn AddDllDirectory(NewDirectory: *const u16) -> *mut std::ffi::c_void;
                }
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = std::ffi::OsStr::new(&dir_str)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
                if !cookie.is_null() {
                    tracing::debug!("AddDllDirectory succeeded for: {}", dir_str);
                } else {
                    tracing::warn!("AddDllDirectory failed for: {}", dir_str);
                }
            }

            if let Err(e) =
                db::odbc_register::ensure_odbc_driver_registered(driver_name, &driver_dll)
            {
                if required {
                    tracing::error!(
                        "Failed to register required ODBC driver '{}': {}. Application may not function correctly.",
                        driver_name,
                        e
                    );
                } else {
                    tracing::warn!(
                        "Optional ODBC driver '{}' registration failed (may need admin): {}",
                        driver_name,
                        e
                    );
                }
            } else {
                tracing::info!("ODBC driver '{}' registered successfully", driver_name);
            }
        }
        None => {
            if required {
                tracing::warn!(
                    "No bundled ODBC driver found for '{}' (env: {}, candidates: {:?})",
                    driver_name,
                    env_var,
                    candidates
                );
            } else {
                tracing::debug!(
                    "No bundled ODBC driver found for optional driver '{}' (env: {}, candidates: {:?})",
                    driver_name,
                    env_var,
                    candidates
                );
            }
        }
    }
}

/// On Windows, add the directory containing aci.dll to PATH so ODPI can load the
/// ShenTong native client via `LoadLibraryA("aci.dll")`.
/// Also sets TNS_ADMIN so ACI can find tnsnames.aci / sqlnet.aci.
#[cfg(windows)]
fn inject_shentong_aci_path() {
    let candidates = ["drivers/shentong/windows", "../drivers/shentong/windows"];

    for dir in &candidates {
        let p = std::path::Path::new(dir).join("aci.dll");
        if p.exists() {
            if let Some(abs_dir) = canonicalize_dll_path(std::path::Path::new(dir)) {
                let current_path = std::env::var("PATH").unwrap_or_default();
                if !current_path
                    .split(';')
                    .any(|p| p.eq_ignore_ascii_case(&abs_dir))
                {
                    std::env::set_var("PATH", format!("{};{}", abs_dir, current_path));
                    tracing::debug!("Added ShenTong ACI directory to PATH: {}", abs_dir);
                }
                // Set TNS_ADMIN so ACI can locate tnsnames.aci and sqlnet.aci
                if std::env::var("TNS_ADMIN").unwrap_or_default().is_empty() {
                    std::env::set_var("TNS_ADMIN", &abs_dir);
                    tracing::debug!("Set TNS_ADMIN = {}", abs_dir);
                }
                tracing::info!("ShenTong aci.dll found at: {}", abs_dir);
                return;
            }
        }
    }
    tracing::debug!("No bundled aci.dll found for ShenTong native connector");
}

/// Start the Axum server on the given port (or default from env/3000). Returns the bound address.
pub async fn start_server(port: Option<u16>) -> Result<SocketAddr> {
    dotenv::dotenv().ok();

    // On Windows, add aci.dll directory to PATH so ODPI can load the ShenTong native client.
    #[cfg(windows)]
    inject_shentong_aci_path();

    // On Windows, register bundled ODBC drivers so Driver Manager can resolve them by name.
    #[cfg(windows)]
    {
        // Required drivers
        register_bundled_driver(
            db::odbc_register::DM8_DRIVER_NAME,
            "DM8_DRIVER_PATH",
            db::odbc_register::DM8_DRIVER_CANDIDATES,
            true,
        );

        // Optional drivers
        let optional_drivers = [
            (
                db::odbc_register::KINGBASE_DRIVER_NAME,
                "KINGBASE_ODBC_DRIVER_PATH",
                db::odbc_register::KINGBASE_DRIVER_CANDIDATES,
            ),
            (
                db::odbc_register::SHENTONG_DRIVER_NAME,
                "SHENTONG_ODBC_DRIVER_PATH",
                db::odbc_register::SHENTONG_DRIVER_CANDIDATES,
            ),
            (
                db::odbc_register::POSTGRESQL_DRIVER_NAME,
                "POSTGRESQL_ODBC_DRIVER_PATH",
                db::odbc_register::POSTGRESQL_DRIVER_CANDIDATES,
            ),
        ];

        for (driver_name, env_var, candidates) in optional_drivers {
            register_bundled_driver(driver_name, env_var, candidates, false);
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
