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

#[derive(Debug, Deserialize)]
pub struct SchemaQuery {
    #[serde(default)]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
}

pub async fn list_schemas(Json(query): Json<SchemaQuery>) -> ApiResult<Vec<String>> {
    let config = ConnectionConfig {
        db_type: query.db_type,
        host: query.host,
        port: query.port,
        username: query.username,
        password: query.password,
        schema: query.schema,
        export_schema: None,
    };

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
    let config = ConnectionConfig {
        db_type: query.db_type,
        host: query.host,
        port: query.port,
        username: query.username,
        password: query.password,
        schema: query.schema.clone(),
        export_schema: None,
    };

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
}

pub async fn get_table_details_handler(
    Path(table): Path<String>,
    Json(query): Json<TableDetailsQuery>,
) -> ApiResult<TableDetails> {
    let config = ConnectionConfig {
        db_type: query.db_type,
        host: query.host,
        port: query.port,
        username: query.username,
        password: query.password,
        schema: query.schema.clone(),
        export_schema: None,
    };

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
