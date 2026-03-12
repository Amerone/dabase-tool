use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::env;

use crate::{
    api::response::{self, ApiResult},
    api::AppState,
    config_store::{NamedStoredConnection, StoredConnection},
    models::{
        ConfigSource, ConnectionConfig, DbType, NamedConnectionResponse, StoredConnectionResponse,
    },
};

#[derive(Debug, Deserialize)]
pub struct NamedConnectionRequest {
    pub name: String,
    pub config: ConnectionConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct GetConnectionQuery {
    pub db_type: Option<DbType>,
}

pub async fn get_connection(
    State(state): State<AppState>,
    Query(query): Query<GetConnectionQuery>,
) -> ApiResult<StoredConnectionResponse> {
    let store = state.config_store.clone();
    let requested_db_type = query.db_type.clone();
    match tokio::task::spawn_blocking(move || store.get_default_for(requested_db_type)).await {
        Ok(Ok(Some(stored))) => response::ok(to_response(stored)),
        Ok(Ok(None)) => match env_connection_config(query.db_type) {
            Ok(config) => response::ok(StoredConnectionResponse {
                config: redact_password(config),
                source: ConfigSource::Env,
                updated_at: None,
            }),
            Err(e) => response::err(
                StatusCode::NOT_FOUND,
                format!("No saved connection and failed to read env: {}", e),
            ),
        },
        Ok(Err(_)) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to read saved config",
        ),
        Err(_) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to read saved config",
        ),
    }
}

pub async fn save_connection(
    State(state): State<AppState>,
    Json(config): Json<ConnectionConfig>,
) -> ApiResult<StoredConnectionResponse> {
    if let Err(e) = config.validate() {
        return response::err(
            StatusCode::BAD_REQUEST,
            format!("Invalid connection config: {}", e),
        );
    }

    let store = state.config_store.clone();
    match tokio::task::spawn_blocking(move || store.upsert_default(&config)).await {
        Ok(Ok(stored)) => response::ok(to_response(stored)),
        Ok(Err(_)) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save connection",
        ),
        Err(_) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save connection",
        ),
    }
}

pub async fn list_connections(
    State(state): State<AppState>,
) -> ApiResult<Vec<NamedConnectionResponse>> {
    let store = state.config_store.clone();
    match tokio::task::spawn_blocking(move || store.list_connections()).await {
        Ok(Ok(items)) => {
            let data = items.into_iter().map(to_named_response).collect::<Vec<_>>();
            response::ok(data)
        }
        Ok(Err(_)) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to list connections",
        ),
        Err(_) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to list connections",
        ),
    }
}

pub async fn save_named_connection(
    State(state): State<AppState>,
    Json(req): Json<NamedConnectionRequest>,
) -> ApiResult<NamedConnectionResponse> {
    if req.name.trim().is_empty() {
        return response::err(StatusCode::BAD_REQUEST, "Connection name cannot be empty");
    }

    if let Err(e) = req.config.validate() {
        return response::err(
            StatusCode::BAD_REQUEST,
            format!("Invalid connection config: {}", e),
        );
    }

    let store = state.config_store.clone();
    match tokio::task::spawn_blocking(move || store.upsert_named(&req.name, &req.config)).await {
        Ok(Ok(stored)) => response::ok(to_named_response(stored)),
        Ok(Err(_)) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save named connection",
        ),
        Err(_) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save named connection",
        ),
    }
}

pub async fn delete_connection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<bool> {
    let store = state.config_store.clone();
    match tokio::task::spawn_blocking(move || store.delete_connection_by_id(id)).await {
        Ok(Ok(deleted)) => response::ok(deleted),
        Ok(Err(_)) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to delete connection",
        ),
        Err(_) => response::err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to delete connection",
        ),
    }
}

fn env_connection_config(requested_db_type: Option<DbType>) -> Result<ConnectionConfig, String> {
    let db_type = match env::var("DATABASE_TYPE")
        .unwrap_or_else(|_| "dm8".to_string())
        .trim()
        .to_lowercase()
        .as_str()
    {
        "dm8" => DbType::Dm8,
        "mysql" => DbType::Mysql,
        "kingbase" => DbType::Kingbase,
        "shentong" => DbType::Shentong,
        other => return Err(format!("DATABASE_TYPE '{}' is not supported", other)),
    };

    let host = env::var("DATABASE_HOST").map_err(|_| "DATABASE_HOST not set".to_string())?;
    let port = env::var("DATABASE_PORT")
        .map_err(|_| "DATABASE_PORT not set".to_string())
        .and_then(|v| {
            v.parse::<u16>()
                .map_err(|_| "DATABASE_PORT is not a valid u16".to_string())
        })?;
    let username =
        env::var("DATABASE_USERNAME").map_err(|_| "DATABASE_USERNAME not set".to_string())?;
    let password =
        env::var("DATABASE_PASSWORD").map_err(|_| "DATABASE_PASSWORD not set".to_string())?;
    let schema = env::var("DATABASE_SCHEMA").map_err(|_| "DATABASE_SCHEMA not set".to_string())?;

    let config = ConnectionConfig {
        db_type,
        host,
        port,
        username,
        password,
        schema,
        export_schema: None,
    };

    if let Some(requested) = requested_db_type {
        if config.db_type != requested {
            return Err(format!(
                "No saved connection for requested db_type and env DATABASE_TYPE is {:?}",
                config.db_type
            ));
        }
    }

    Ok(config)
}

fn to_response(stored: StoredConnection) -> StoredConnectionResponse {
    StoredConnectionResponse {
        config: redact_password(stored.config),
        source: stored.source,
        updated_at: stored.updated_at,
    }
}

fn to_named_response(stored: NamedStoredConnection) -> NamedConnectionResponse {
    NamedConnectionResponse {
        id: stored.id,
        name: stored.name,
        config: redact_password(stored.config),
        updated_at: stored.updated_at,
    }
}

fn redact_password(mut config: ConnectionConfig) -> ConnectionConfig {
    config.password.clear();
    config
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
        std::env::set_var("DATABASE_TYPE", "dm8");

        let state = AppState {
            config_store: Arc::new(store),
        };

        let response = get_connection(State(state.clone()), Query(GetConnectionQuery::default()))
            .await
            .unwrap();
        let data = response.0.data.unwrap();
        assert_eq!(data.source, ConfigSource::Env);
        assert_eq!(data.config.host, "env-host");
        assert_eq!(data.config.port, 1234);
        assert_eq!(data.config.password, "");
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
        assert_eq!(saved_data.config.password, "");

        let get_res = get_connection(State(state), Query(GetConnectionQuery::default()))
            .await
            .unwrap();
        let data = get_res.0.data.unwrap();
        assert_eq!(data.source, ConfigSource::Sqlite);
        assert_eq!(data.config.username, "user1");
        assert_eq!(data.config.password, "");
        assert!(data.updated_at.is_some());
    }

    #[tokio::test]
    async fn get_connection_uses_db_type_filter() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();
        let state = AppState {
            config_store: Arc::new(store),
        };

        let dm8 = ConnectionConfig {
            db_type: DbType::Dm8,
            host: "dm8-host".to_string(),
            port: 5236,
            username: "dm8-user".to_string(),
            password: "dm8-pass".to_string(),
            schema: "dm8-schema".to_string(),
            export_schema: None,
        };
        let mysql = ConnectionConfig {
            db_type: DbType::Mysql,
            host: "mysql-host".to_string(),
            port: 3306,
            username: "mysql-user".to_string(),
            password: "mysql-pass".to_string(),
            schema: "mysql-schema".to_string(),
            export_schema: None,
        };

        let _ = save_connection(State(state.clone()), Json(dm8))
            .await
            .unwrap();
        let _ = save_connection(State(state.clone()), Json(mysql))
            .await
            .unwrap();

        let dm8_res = get_connection(
            State(state),
            Query(GetConnectionQuery {
                db_type: Some(DbType::Dm8),
            }),
        )
        .await
        .unwrap();
        let data = dm8_res.0.data.unwrap();
        assert_eq!(data.config.db_type, DbType::Dm8);
        assert_eq!(data.config.host, "dm8-host");
    }

    #[tokio::test]
    async fn save_list_delete_named_connection_flow() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();
        let state = AppState {
            config_store: Arc::new(store),
        };

        let req = NamedConnectionRequest {
            name: "mysql-dev".to_string(),
            config: ConnectionConfig {
                db_type: DbType::Mysql,
                host: "127.0.0.1".to_string(),
                port: 3306,
                username: "root".to_string(),
                password: "secret".to_string(),
                schema: "demo".to_string(),
                export_schema: None,
            },
        };

        let save_res = save_named_connection(State(state.clone()), Json(req))
            .await
            .unwrap();
        let saved = save_res.0.data.unwrap();
        assert!(saved.id > 0);
        assert_eq!(saved.name, "mysql-dev");

        let list_res = list_connections(State(state.clone())).await.unwrap();
        let list = list_res.0.data.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "mysql-dev");
        assert_eq!(list[0].config.password, "");

        let del_res = delete_connection(State(state), Path(saved.id))
            .await
            .unwrap();
        assert_eq!(del_res.0.data, Some(true));
    }

    #[tokio::test]
    async fn save_connection_returns_bad_request_for_invalid_payload() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();
        let state = AppState {
            config_store: Arc::new(store),
        };

        let invalid = ConnectionConfig {
            db_type: DbType::Dm8,
            host: "127.0.0.1".to_string(),
            port: 5236,
            username: "SYSDBA".to_string(),
            password: "".to_string(),
            schema: "SYSDBA".to_string(),
            export_schema: None,
        };

        let err = save_connection(State(state), Json(invalid))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1 .0.success, false);
        assert!(err
            .1
             .0
            .error
            .unwrap_or_default()
            .contains("Invalid connection config"));
    }

    #[tokio::test]
    async fn save_named_connection_returns_bad_request_for_empty_name() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();
        let state = AppState {
            config_store: Arc::new(store),
        };

        let req = NamedConnectionRequest {
            name: "   ".to_string(),
            config: ConnectionConfig {
                db_type: DbType::Dm8,
                host: "127.0.0.1".to_string(),
                port: 5236,
                username: "SYSDBA".to_string(),
                password: "SYSDBA".to_string(),
                schema: "SYSDBA".to_string(),
                export_schema: None,
            },
        };

        let err = save_named_connection(State(state), Json(req))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1 .0.success, false);
        assert!(err
            .1
             .0
            .error
            .unwrap_or_default()
            .contains("Connection name cannot be empty"));
    }
}
