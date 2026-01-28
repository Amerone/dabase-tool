pub mod connection;
pub mod schema;
pub mod export;
pub mod config;

use axum::{
    routing::{get, post},
    Router,
};
use crate::config_store::ConfigStore;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

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
        .route("/api/tables/:table/details", get(schema::get_table_details_handler))
        .route("/api/export/ddl", post(export::export_ddl))
        .route("/api/export/data", post(export::export_data))
        .route("/api/config/connection", get(config::get_connection).post(config::save_connection))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}
