use axum::{extract::Json, http::StatusCode};
use chrono::Local;
use std::path::PathBuf;

use crate::{
    db::connection::ConnectionPool,
    export::data::export_schema_data,
    export::ddl::{export_schema_ddl, TriggerTerminator},
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

fn resolve_compat(value: Option<&str>) -> TriggerTerminator {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(mode) if mode.eq_ignore_ascii_case("script") => TriggerTerminator::Script,
        Some(mode) if mode.eq_ignore_ascii_case("datagrip-script") => {
            TriggerTerminator::DataGripScript
        }
        Some(mode) if mode.eq_ignore_ascii_case("datagrip") => TriggerTerminator::DataGrip,
        _ => TriggerTerminator::DataGrip,
    }
}

/// Replace characters that are unsafe in file paths with underscores.
fn sanitize_filename_part(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn exports_dir() -> Result<PathBuf, String> {
    let home =
        dirs::home_dir().ok_or_else(|| "Unable to determine home directory".to_string())?;
    let dir = home.join(".amarone").join("exports");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create exports directory {:?}: {}", dir, e))?;
    Ok(dir)
}

fn format_export_filename(
    source: &str,
    target: &str,
    kind: &str,
    suffix: &str,
) -> Result<PathBuf, String> {
    let dir = exports_dir()?;
    Ok(dir.join(format!(
        "{}_to_{}_{}_{}.sql",
        sanitize_filename_part(source),
        sanitize_filename_part(target),
        kind,
        suffix
    )))
}

fn format_error_chain(err: &anyhow::Error) -> String {
    format!("{:#}", err)
}

fn connect_from_request(
    req: &mut ExportRequest,
) -> Result<(ConnectionPool, String, String), String> {
    let config = ConnectionConfig {
        host: std::mem::take(&mut req.config.host),
        port: req.config.port,
        username: std::mem::take(&mut req.config.username),
        password: std::mem::take(&mut req.config.password),
        schema: req.config.schema.clone(),
        export_schema: req.config.export_schema.clone(),
    };

    let source_schema = config.schema.clone();
    let target_schema = resolve_target_schema(
        &source_schema,
        req.export_schema
            .as_deref()
            .or(config.export_schema.as_deref()),
    );

    let pool =
        ConnectionPool::new(config).map_err(|e| format!("Failed to create connection: {e}"))?;

    Ok((pool, source_schema, target_schema))
}

#[cfg(test)]
mod tests {
    use super::{
        format_error_chain, format_export_filename, resolve_compat, resolve_target_schema,
        sanitize_filename_part,
    };
    use crate::export::ddl::TriggerTerminator;

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
        let path = format_export_filename("SRC", "TGT", "ddl", "20260130_120000_000").unwrap();
        assert!(path.is_absolute(), "export path must be absolute");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "SRC_to_TGT_ddl_20260130_120000_000.sql");
    }

    #[test]
    fn format_export_filename_sanitizes_special_chars() {
        let path = format_export_filename("../evil", "a;b", "ddl", "ts").unwrap();
        assert!(path.is_absolute(), "export path must be absolute");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "___evil_to_a_b_ddl_ts.sql");
    }

    #[test]
    fn sanitize_filename_part_keeps_safe_chars() {
        assert_eq!(sanitize_filename_part("Hello_World-1"), "Hello_World-1");
    }

    #[test]
    fn sanitize_filename_part_replaces_dots_slashes() {
        assert_eq!(sanitize_filename_part("../foo.bar"), "___foo_bar");
    }

    #[test]
    fn format_error_chain_includes_contexts() {
        let err = anyhow::anyhow!("root cause")
            .context("middle context")
            .context("top context");
        let rendered = format_error_chain(&err);
        assert!(rendered.contains("top context"));
        assert!(rendered.contains("middle context"));
        assert!(rendered.contains("root cause"));
    }

    #[test]
    fn resolve_compat_defaults_to_datagrip() {
        let mode = resolve_compat(None);
        assert_eq!(mode, TriggerTerminator::DataGrip);
    }

    #[test]
    fn resolve_compat_datagrip_script_maps_to_datagrip_script() {
        let mode = resolve_compat(Some("datagrip-script"));
        assert_eq!(mode, TriggerTerminator::DataGripScript);
    }
}

pub async fn export_ddl(
    Json(mut req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportResponse>>, StatusCode> {
    let (pool, source_schema, target_schema) = match connect_from_request(&mut req) {
        Ok(v) => v,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };

    let connection = match pool.get_connection() {
        Ok(conn) => conn,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to get connection: {e}"
            ))))
        }
    };

    let date_suffix = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let output_path = match format_export_filename(&source_schema, &target_schema, "ddl", &date_suffix) {
        Ok(p) => p,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };

    match export_schema_ddl(
        &connection,
        &source_schema,
        &target_schema,
        &req.tables,
        &output_path,
        req.drop_existing,
        resolve_compat(req.export_compat.as_deref()),
    ) {
        Ok(_) => Ok(Json(ApiResponse::success(ExportResponse {
            success: true,
            message: "DDL exported successfully".to_string(),
            file_path: Some(output_path.to_string_lossy().to_string()),
        }))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to export DDL: {}",
            format_error_chain(&e)
        )))),
    }
}

pub async fn export_data(
    Json(mut req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportResponse>>, StatusCode> {
    let (pool, source_schema, target_schema) = match connect_from_request(&mut req) {
        Ok(v) => v,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };

    let connection = match pool.get_connection() {
        Ok(conn) => conn,
        Err(e) => {
            return Ok(Json(ApiResponse::error(format!(
                "Failed to get connection: {e}"
            ))))
        }
    };

    let date_suffix = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let output_path = match format_export_filename(&source_schema, &target_schema, "data", &date_suffix) {
        Ok(p) => p,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };
    let batch_size = req.batch_size.unwrap_or(1000);

    match export_schema_data(
        &connection,
        &source_schema,
        &target_schema,
        &req.tables,
        &output_path,
        batch_size,
        req.include_row_counts,
    ) {
        Ok(_) => Ok(Json(ApiResponse::success(ExportResponse {
            success: true,
            message: "Data exported successfully".to_string(),
            file_path: Some(output_path.to_string_lossy().to_string()),
        }))),
        Err(e) => Ok(Json(ApiResponse::error(format!(
            "Failed to export data: {}",
            format_error_chain(&e)
        )))),
    }
}

pub async fn get_export_directory() -> Result<Json<ApiResponse<String>>, StatusCode> {
    match exports_dir() {
        Ok(dir) => Ok(Json(ApiResponse::success(
            dir.to_string_lossy().to_string(),
        ))),
        Err(e) => Ok(Json(ApiResponse::error(e))),
    }
}
