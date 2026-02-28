pub mod config;
pub mod connection;
pub mod export;
pub mod schema;

use crate::config_store::ConfigStore;
use axum::{
    http::{header, Method},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub config_store: Arc<ConfigStore>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/connection/test", post(connection::test_connection))
        .route("/api/schemas", get(schema::list_schemas))
        .route("/api/tables", get(schema::list_tables))
        .route(
            "/api/tables/:table/details",
            get(schema::get_table_details_handler),
        )
        .route("/api/export/ddl", post(export::export_ddl))
        .route("/api/export/data", post(export::export_data))
        .route("/api/export/directory", get(export::get_export_directory))
        .route(
            "/api/config/connection",
            get(config::get_connection).post(config::save_connection),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _| {
                    let Ok(s) = origin.to_str() else {
                        return false;
                    };
                    s.starts_with("http://localhost:")
                        || s.starts_with("http://127.0.0.1:")
                        || s == "tauri://localhost"
                        || s == "https://tauri.localhost"
                    || s == "http://tauri.localhost"
                }))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE]),
        )
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}
