use axum::{extract::Json, http::StatusCode};
use std::path::PathBuf;

use crate::{
    db::connection::ConnectionPool,
    export::{data::export_schema_data, ddl::export_schema_ddl},
    models::{ApiResponse, ConnectionConfig, ExportRequest, ExportResponse},
};

pub async fn export_ddl(
    Json(req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportResponse>>, StatusCode> {
    let config = ConnectionConfig {
        host: req.config.host,
        port: req.config.port,
        username: req.config.username,
        password: req.config.password,
        schema: req.config.schema.clone(),
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

    let output_path = PathBuf::from(format!("exports/{}_ddl.sql", req.config.schema));

    match export_schema_ddl(&connection, &req.config.schema, &req.tables, &output_path) {
        Ok(_) => Ok(Json(ApiResponse::success(ExportResponse {
            success: true,
            message: "DDL exported successfully".to_string(),
            file_path: Some(output_path.to_string_lossy().to_string()),
        }))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to export DDL: {}",
            e
        )))),
    }
}

pub async fn export_data(
    Json(req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportResponse>>, StatusCode> {
    let config = ConnectionConfig {
        host: req.config.host,
        port: req.config.port,
        username: req.config.username,
        password: req.config.password,
        schema: req.config.schema.clone(),
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

    let output_path = PathBuf::from(format!("exports/{}_data.sql", req.config.schema));
    let batch_size = req.batch_size.unwrap_or(1000);

    match export_schema_data(&connection, &req.config.schema, &req.tables, &output_path, batch_size) {
        Ok(_) => Ok(Json(ApiResponse::success(ExportResponse {
            success: true,
            message: "Data exported successfully".to_string(),
            file_path: Some(output_path.to_string_lossy().to_string()),
        }))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to export data: {}",
            e
        )))),
    }
}
