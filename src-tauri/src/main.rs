#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod driver;

use driver::{discover_and_apply, DriverSetup, DriverSource};
use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, serde::Serialize)]
struct DriverInfo {
    path: String,
    source: DriverSource,
    drivers: Vec<PackagedDriverInfo>,
}

#[derive(Clone, serde::Serialize)]
struct PackagedDriverInfo {
    database: String,
    path: String,
    source: DriverSource,
    required: bool,
}

#[derive(Clone)]
struct AppState {
    driver_setup: DriverSetup,
    backend_url: String,
}

#[tauri::command]
fn backend_base_url(state: State<'_, AppState>) -> String {
    state.backend_url.clone()
}

#[tauri::command]
fn driver_info(state: State<'_, AppState>) -> DriverInfo {
    let drivers = state
        .driver_setup
        .drivers
        .iter()
        .map(|driver| PackagedDriverInfo {
            database: driver.database.to_string(),
            path: driver.driver_path.display().to_string(),
            source: driver.source.clone(),
            required: driver.required,
        })
        .collect();

    DriverInfo {
        path: state.driver_setup.primary.driver_path.display().to_string(),
        source: state.driver_setup.primary.source.clone(),
        drivers,
    }
}

#[tauri::command]
async fn choose_export_directory(
    app: tauri::AppHandle,
    initial_directory: Option<String>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("选择导出目录");

        if let Some(directory) = initial_directory
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let path = PathBuf::from(directory);
            if path.exists() {
                dialog = dialog.set_directory(path);
            }
        }

        dialog
            .blocking_pick_folder()
            .map(|folder| {
                folder
                    .into_path()
                    .map(|path| path.to_string_lossy().to_string())
                    .map_err(|err| err.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|err| err.to_string())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            backend_base_url,
            driver_info,
            choose_export_directory
        ])
        .setup(|app| {
            let resolved = match discover_and_apply(app.handle()) {
                Ok(setup) => setup,
                Err(err) => {
                    app.dialog()
                        .message(format!("Failed to locate required database driver: {err}"))
                        .title("Database driver missing")
                        .blocking_show();
                    return Err(err.into());
                }
            };

            dm8_export_backend::init_tracing();
            let bound = tauri::async_runtime::block_on(dm8_export_backend::start_server(Some(0)))?;
            let backend_url = format!("http://127.0.0.1:{}", bound.port());

            app.manage(AppState {
                driver_setup: resolved,
                backend_url,
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DM8 Export Tool");
}
