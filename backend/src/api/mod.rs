pub mod capability;
pub mod config;
pub mod connection;
pub mod export;
pub mod response;
pub mod schema;

use crate::config_store::ConfigStore;
use axum::{
    http::{header, Method},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    pub config_store: Arc<ConfigStore>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/connection/test", post(connection::test_connection))
        .route("/api/schemas", post(schema::list_schemas))
        .route("/api/tables", post(schema::list_tables))
        .route(
            "/api/tables/details/batch",
            post(schema::get_table_details_batch_handler),
        )
        .route(
            "/api/tables/:table/details",
            post(schema::get_table_details_handler),
        )
        .route("/api/export/ddl", post(export::export_ddl))
        .route("/api/export/data", post(export::export_data))
        .route(
            "/api/export/capabilities",
            get(capability::get_export_capabilities),
        )
        .route("/api/export/directory", get(export::get_export_directory))
        .route(
            "/api/config/connection",
            get(config::get_connection).post(config::save_connection),
        )
        .route(
            "/api/config/connections",
            get(config::list_connections).post(config::save_named_connection),
        )
        .route(
            "/api/config/connections/:id",
            axum::routing::delete(config::delete_connection),
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
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE]),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}
