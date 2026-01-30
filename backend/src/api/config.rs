use axum::{extract::State, http::StatusCode, Json};
use std::env;

use crate::{
    api::AppState,
    config_store::StoredConnection,
    models::{ApiResponse, ConfigSource, ConnectionConfig, StoredConnectionResponse},
};

pub async fn get_connection(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<StoredConnectionResponse>>, StatusCode> {
    match state.config_store.get_default() {
        Ok(Some(stored)) => Ok(Json(ApiResponse::success(to_response(stored)))),
        Ok(None) => match env_connection_config() {
            Ok(config) => Ok(Json(ApiResponse::success(StoredConnectionResponse {
                config,
                source: ConfigSource::Env,
                updated_at: None,
            }))),
            Err(e) => Ok(Json(ApiResponse::error(format!(
                "No saved connection and failed to read env: {}",
                e
            )))),
        },
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to read saved config: {}",
            e
        )))),
    }
}

pub async fn save_connection(
    State(state): State<AppState>,
    Json(config): Json<ConnectionConfig>,
) -> Result<Json<ApiResponse<StoredConnectionResponse>>, StatusCode> {
    if let Err(e) = config.validate() {
        return Ok(Json(ApiResponse::error(format!(
            "Invalid connection config: {}",
            e
        ))));
    }

    match state.config_store.upsert_default(&config) {
        Ok(stored) => Ok(Json(ApiResponse::success(to_response(stored)))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to save connection: {}",
            e
        )))),
    }
}

fn env_connection_config() -> Result<ConnectionConfig, String> {
    let host = env::var("DATABASE_HOST").map_err(|_| "DATABASE_HOST not set".to_string())?;
    let port = env::var("DATABASE_PORT")
        .map_err(|_| "DATABASE_PORT not set".to_string())
        .and_then(|v| v.parse::<u16>().map_err(|_| "DATABASE_PORT is not a valid u16".to_string()))?;
    let username =
        env::var("DATABASE_USERNAME").map_err(|_| "DATABASE_USERNAME not set".to_string())?;
    let password =
        env::var("DATABASE_PASSWORD").map_err(|_| "DATABASE_PASSWORD not set".to_string())?;
    let schema = env::var("DATABASE_SCHEMA").map_err(|_| "DATABASE_SCHEMA not set".to_string())?;

    Ok(ConnectionConfig {
        host,
        port,
        username,
        password,
        schema,
        export_schema: None,
    })
}

fn to_response(stored: StoredConnection) -> StoredConnectionResponse {
    StoredConnectionResponse {
        config: stored.config,
        source: stored.source,
        updated_at: stored.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    use crate::config_store::ConfigStore;

    #[tokio::test]
    async fn get_returns_env_when_no_saved() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        std::env::set_var("DATABASE_HOST", "env-host");
        std::env::set_var("DATABASE_PORT", "1234");
        std::env::set_var("DATABASE_USERNAME", "env-user");
        std::env::set_var("DATABASE_PASSWORD", "env-pass");
        std::env::set_var("DATABASE_SCHEMA", "env-schema");

        let state = AppState {
            config_store: Arc::new(store),
        };

        let response = get_connection(State(state.clone())).await.unwrap();
        let data = response.0.data.unwrap();
        assert_eq!(data.source, ConfigSource::Env);
        assert_eq!(data.config.host, "env-host");
        assert_eq!(data.config.port, 1234);
    }

    #[tokio::test]
    async fn post_then_get_returns_sqlite_source() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();
        let state = AppState {
            config_store: Arc::new(store),
        };

        let save_body = json!({
            "host": "sqlite-host",
            "port": 5236,
            "username": "user1",
            "password": "pass1",
            "schema": "schema1"
        });

        let save_res = save_connection(
            State(state.clone()),
            Json(serde_json::from_value(save_body.clone()).unwrap()),
        )
        .await
        .unwrap();

        let saved_data = save_res.0.data.unwrap();
        assert_eq!(saved_data.source, ConfigSource::Sqlite);
        assert_eq!(saved_data.config.host, "sqlite-host");

        let get_res = get_connection(State(state)).await.unwrap();
        let data = get_res.0.data.unwrap();
        assert_eq!(data.source, ConfigSource::Sqlite);
        assert_eq!(data.config.username, "user1");
        assert!(data.updated_at.is_some());
    }
}
