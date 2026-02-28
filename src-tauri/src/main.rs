#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod driver;

use driver::{discover_and_apply, DriverSource, ResolvedDriver};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, serde::Serialize)]
struct DriverInfo {
    path: String,
    source: DriverSource,
}

#[derive(Clone)]
struct AppState {
    driver: ResolvedDriver,
    backend_url: String,
}

#[tauri::command]
fn backend_base_url(state: State<'_, AppState>) -> String {
    state.backend_url.clone()
}

#[tauri::command]
fn driver_info(state: State<'_, AppState>) -> DriverInfo {
    DriverInfo {
        path: state.driver.driver_path.display().to_string(),
        source: state.driver.source.clone(),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![backend_base_url, driver_info])
        .setup(|app| {
            let resolved = match discover_and_apply(app.handle()) {
                Ok(driver) => driver,
                Err(err) => {
                    app.dialog()
                        .message(format!("Failed to locate DM8 ODBC driver: {err}"))
                        .title("DM8 driver missing")
                        .blocking_show();
                    return Err(err.into());
                }
            };

            dm8_export_backend::init_tracing();
            let bound = tauri::async_runtime::block_on(dm8_export_backend::start_server(Some(0)))?;
            let backend_url = format!("http://127.0.0.1:{}", bound.port());

            app.manage(AppState {
                driver: resolved,
                backend_url,
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DM8 Export Tool");
}
