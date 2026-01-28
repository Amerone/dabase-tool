use axum::{
    extract::{Json, Path, Query},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    db::{
        connection::ConnectionPool,
        schema::{get_table_details, get_tables},
    },
    models::{ApiResponse, ConnectionConfig, Table, TableDetails},
};

#[derive(Debug, Deserialize)]
pub struct SchemaQuery {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
}

pub async fn list_schemas() -> Json<ApiResponse<Vec<String>>> {
    Json(ApiResponse::error(
        "List schemas not implemented yet".to_string(),
    ))
}

pub async fn list_tables(
    Query(query): Query<SchemaQuery>,
) -> Result<Json<ApiResponse<Vec<Table>>>, StatusCode> {
    let config = ConnectionConfig {
        host: query.host,
        port: query.port,
        username: query.username,
        password: query.password,
        schema: query.schema.clone(),
    };

    let pool = match ConnectionPool::new(config) {
        Ok(pool) => pool,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to create connection: {}",
                e
            ))))
        }
    };

    let connection = match pool.get_connection() {
        Ok(conn) => conn,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to get connection: {}",
                e
            ))))
        }
    };

    match get_tables(&connection, &query.schema) {
        Ok(tables) => Ok(Json(ApiResponse::success(tables))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to get tables: {}",
            e
        )))),
    }
}

pub async fn get_table_details_handler(
    Path(table): Path<String>,
    Query(query): Query<SchemaQuery>,
) -> Result<Json<ApiResponse<TableDetails>>, StatusCode> {
    let config = ConnectionConfig {
        host: query.host,
        port: query.port,
        username: query.username,
        password: query.password,
        schema: query.schema.clone(),
    };

    let pool = match ConnectionPool::new(config) {
        Ok(pool) => pool,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to create connection: {}",
                e
            ))))
        }
    };

    let connection = match pool.get_connection() {
        Ok(conn) => conn,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to get connection: {}",
                e
            ))))
        }
    };

    match get_table_details(&connection, &query.schema, &table) {
        Ok(details) => Ok(Json(ApiResponse::success(details))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to get table details: {}",
            e
        )))),
    }
}
