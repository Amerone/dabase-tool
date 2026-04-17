use axum::{
    extract::{Json, Path},
    http::StatusCode,
};
use serde::Deserialize;
use tracing::error;

use crate::{
    api::response::{self, ApiResult},
    db::service,
    models::{ConnectionConfig, DbType, ErrorCode, Table, TableDetails},
};

const MAX_BATCH_TABLES: usize = 200;

#[derive(Debug, Deserialize)]
pub struct SchemaQuery {
    #[serde(default)]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
    pub database: Option<String>,
    #[serde(default)]
    pub force_refresh: bool,
}

impl SchemaQuery {
    fn into_config(self) -> ConnectionConfig {
        ConnectionConfig {
            db_type: self.db_type,
            host: self.host,
            port: self.port,
            username: self.username,
            password: self.password,
            schema: self.schema,
            export_schema: None,
            database: self.database,
        }
    }
}

pub async fn list_schemas(Json(query): Json<SchemaQuery>) -> ApiResult<Vec<String>> {
    let config = query.into_config();

    match service::list_schemas(&config).await {
        Ok(schemas) => response::ok(schemas),
        Err(e) => {
            error!(error = ?e, "Failed to get schemas");
            response::err_with_code(
                StatusCode::BAD_REQUEST,
                "Failed to get schemas",
                ErrorCode::DatabaseQuery,
            )
        }
    }
}

pub async fn list_tables(Json(query): Json<SchemaQuery>) -> ApiResult<Vec<Table>> {
    if query.force_refresh {
        service::clear_metadata_caches();
    }
    let config = query.into_config();

    match service::list_tables(&config).await {
        Ok(tables) => response::ok(tables),
        Err(e) => {
            error!(error = ?e, "Failed to get tables");
            response::err_with_code(
                StatusCode::BAD_REQUEST,
                "Failed to get tables",
                ErrorCode::DatabaseQuery,
            )
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TableDetailsQuery {
    #[serde(default)]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
    pub table_schema: String,
    pub database: Option<String>,
    #[serde(default)]
    pub force_refresh: bool,
}

impl TableDetailsQuery {
    fn to_config(&self) -> ConnectionConfig {
        ConnectionConfig {
            db_type: self.db_type.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            schema: self.schema.clone(),
            export_schema: None,
            database: self.database.clone(),
        }
    }
}

pub async fn get_table_details_handler(
    Path(table): Path<String>,
    Json(query): Json<TableDetailsQuery>,
) -> ApiResult<TableDetails> {
    if query.force_refresh {
        service::clear_metadata_caches();
    }
    let config = query.to_config();

    match service::get_table_details(&config, &query.table_schema, &table).await {
        Ok(details) => response::ok(details),
        Err(e) => {
            error!(error = ?e, table = %table, schema = %query.table_schema, "Failed to get table details");
            response::err_with_code(
                StatusCode::BAD_REQUEST,
                "Failed to get table details",
                ErrorCode::DatabaseQuery,
            )
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TableDetailsBatchQuery {
    #[serde(default)]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
    pub table_schema: String,
    #[serde(default)]
    pub tables: Vec<String>,
    pub database: Option<String>,
    #[serde(default)]
    pub force_refresh: bool,
}

impl TableDetailsBatchQuery {
    fn to_config(&self) -> ConnectionConfig {
        ConnectionConfig {
            db_type: self.db_type.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            schema: self.schema.clone(),
            export_schema: None,
            database: self.database.clone(),
        }
    }
}

pub async fn get_table_details_batch_handler(
    Json(query): Json<TableDetailsBatchQuery>,
) -> ApiResult<Vec<TableDetails>> {
    if query.force_refresh {
        service::clear_metadata_caches();
    }

    let tables: Vec<String> = query
        .tables
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if tables.is_empty() {
        return response::err_with_code(
            StatusCode::BAD_REQUEST,
            "tables must not be empty",
            ErrorCode::ValidationFailed,
        );
    }

    if tables.len() > MAX_BATCH_TABLES {
        return response::err_with_code(
            StatusCode::BAD_REQUEST,
            format!("tables must not exceed {}", MAX_BATCH_TABLES),
            ErrorCode::ValidationFailed,
        );
    }

    let config = query.to_config();
    match service::get_table_details_batch(&config, &query.table_schema, &tables).await {
        Ok(details) => response::ok(details),
        Err(e) => {
            error!(
                error = ?e,
                schema = %query.table_schema,
                table_count = tables.len(),
                "Failed to batch get table details"
            );
            response::err_with_code(
                StatusCode::BAD_REQUEST,
                "Failed to get table details batch",
                ErrorCode::DatabaseQuery,
            )
        }
    }
}
