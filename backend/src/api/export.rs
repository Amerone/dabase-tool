use axum::{extract::Json, http::StatusCode};
use std::path::PathBuf;

use crate::{
    db::connection::ConnectionPool,
    export::{data::export_schema_data, ddl::export_schema_ddl},
    models::{ApiResponse, ConnectionConfig, ExportRequest, ExportResponse},
};

fn normalize_schema_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn resolve_target_schema(source: &str, export_schema: Option<&str>) -> String {
    normalize_schema_value(export_schema).unwrap_or_else(|| source.trim().to_string())
}

fn format_export_filename(source: &str, target: &str, kind: &str) -> String {
    format!(
        "exports/{}_to_{}_{}.sql",
        source.trim(),
        target.trim(),
        kind
    )
}

#[cfg(test)]
mod tests {
    use super::{format_export_filename, resolve_target_schema};

    #[test]
    fn resolve_target_schema_falls_back_to_source() {
        let target = resolve_target_schema("SYSDBA", None);
        assert_eq!(target, "SYSDBA");
    }

    #[test]
    fn resolve_target_schema_uses_trimmed_value() {
        let target = resolve_target_schema("SYSDBA", Some("  APP  "));
        assert_eq!(target, "APP");
    }

    #[test]
    fn format_export_filename_includes_source_and_target() {
        let name = format_export_filename("SRC", "TGT", "ddl");
        assert_eq!(name, "exports/SRC_to_TGT_ddl.sql");
    }
}

pub async fn export_ddl(
    Json(req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportResponse>>, StatusCode> {
    let config = ConnectionConfig {
        host: req.config.host,
        port: req.config.port,
        username: req.config.username,
        password: req.config.password,
        schema: req.config.schema.clone(),
        export_schema: req.config.export_schema.clone(),
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

    let source_schema = req.config.schema.clone();
    let target_schema = resolve_target_schema(
        &source_schema,
        req.export_schema
            .as_deref()
            .or(req.config.export_schema.as_deref()),
    );
    let output_path = PathBuf::from(format_export_filename(&source_schema, &target_schema, "ddl"));

    match export_schema_ddl(
        &connection,
        &source_schema,
        &target_schema,
        &req.tables,
        &output_path,
    ) {
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
        export_schema: req.config.export_schema.clone(),
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

    let source_schema = req.config.schema.clone();
    let target_schema = resolve_target_schema(
        &source_schema,
        req.export_schema
            .as_deref()
            .or(req.config.export_schema.as_deref()),
    );
    let output_path = PathBuf::from(format_export_filename(&source_schema, &target_schema, "data"));
    let batch_size = req.batch_size.unwrap_or(1000);

    match export_schema_data(
        &connection,
        &source_schema,
        &target_schema,
        &req.tables,
        &output_path,
        batch_size,
    ) {
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
