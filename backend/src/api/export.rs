mod context;
mod execution;

use axum::{extract::Json, http::StatusCode};
use chrono::Local;
use std::time::Instant;

use crate::{
    api::response::{self, ApiResult},
    export::orchestrator::{ExportWorkload, LegacyExportOrchestrator, LegacyExportPlan},
    models::{ErrorCode, ExportResponse},
};

use self::{
    context::{
        build_export_context, build_summary, collect_workload_warnings, exports_dir,
        format_export_filename, resolve_compat, resolve_include_row_counts, resolve_target_dialect,
    },
    execution::{execute_data_by_path, execute_ddl_by_path},
};

const DEFAULT_BATCH_SIZE: usize = 1000;
const MAX_BATCH_SIZE: usize = 10_000;

fn resolve_batch_size(batch_size: Option<usize>) -> Result<usize, String> {
    match batch_size {
        Some(size) if size == 0 || size > MAX_BATCH_SIZE => Err(format!(
            "batch_size must be between 1 and {}",
            MAX_BATCH_SIZE
        )),
        Some(size) => Ok(size),
        None => Ok(DEFAULT_BATCH_SIZE),
    }
}

pub async fn export_ddl(
    Json(mut req): Json<crate::models::ExportRequest>,
) -> ApiResult<ExportResponse> {
    let source_db_type = req.config.db_type.clone();
    let target_dialect = resolve_target_dialect(&req);
    let execution_path = match LegacyExportOrchestrator::resolve_execution_path(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Ddl,
    ) {
        Ok(path) => path,
        Err(message) => return response::err(StatusCode::BAD_REQUEST, message),
    };
    let warnings = match collect_workload_warnings(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Ddl,
        req.strict_mode,
    ) {
        Ok(value) => value,
        Err(message) => return response::err(StatusCode::UNPROCESSABLE_ENTITY, message),
    };

    let (config, source_schema, target_schema) = build_export_context(&mut req);

    let date_suffix = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let output_path =
        match format_export_filename(&source_schema, &target_schema, "ddl", &date_suffix) {
            Ok(p) => p,
            Err(e) => return response::err(StatusCode::INTERNAL_SERVER_ERROR, e),
        };

    let plan = LegacyExportPlan {
        source_schema,
        target_schema,
        tables: req.tables,
        output_path,
    };

    let start = Instant::now();
    let result = execute_ddl_by_path(
        execution_path,
        &config,
        &plan,
        req.drop_existing,
        resolve_compat(req.export_compat.as_deref()),
    )
    .await;

    match result {
        Ok(_) => {
            let duration_ms = start.elapsed().as_millis();
            let summary = build_summary(
                ExportWorkload::Ddl,
                execution_path,
                duration_ms,
                None,
                warnings,
            );
            tracing::info!(
                workload = "ddl",
                ?source_db_type,
                ?target_dialect,
                ?execution_path,
                duration_ms,
                output_path = %plan.output_path.display(),
                "Export completed"
            );
            response::ok(ExportResponse {
                success: true,
                message: "DDL exported successfully".to_string(),
                file_path: Some(plan.output_path.to_string_lossy().to_string()),
                summary: Some(summary),
            })
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to export DDL");
            response::err_with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to export DDL",
                ErrorCode::ExportFailed,
            )
        }
    }
}

pub async fn export_data(
    Json(mut req): Json<crate::models::ExportRequest>,
) -> ApiResult<ExportResponse> {
    let source_db_type = req.config.db_type.clone();
    let target_dialect = resolve_target_dialect(&req);
    let execution_path = match LegacyExportOrchestrator::resolve_execution_path(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Data,
    ) {
        Ok(path) => path,
        Err(message) => return response::err(StatusCode::BAD_REQUEST, message),
    };
    let mut warnings = match collect_workload_warnings(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Data,
        req.strict_mode,
    ) {
        Ok(value) => value,
        Err(message) => return response::err(StatusCode::UNPROCESSABLE_ENTITY, message),
    };

    let include_row_counts = match resolve_include_row_counts(
        execution_path,
        req.include_row_counts,
        req.strict_mode,
        &mut warnings,
    ) {
        Ok(value) => value,
        Err(message) => return response::err(StatusCode::UNPROCESSABLE_ENTITY, message),
    };

    let (config, source_schema, target_schema) = build_export_context(&mut req);

    let date_suffix = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let output_path =
        match format_export_filename(&source_schema, &target_schema, "data", &date_suffix) {
            Ok(p) => p,
            Err(e) => return response::err(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
    let batch_size = match resolve_batch_size(req.batch_size) {
        Ok(size) => size,
        Err(message) => return response::err(StatusCode::BAD_REQUEST, message),
    };

    let plan = LegacyExportPlan {
        source_schema,
        target_schema,
        tables: req.tables,
        output_path,
    };

    let start = Instant::now();
    let result = execute_data_by_path(
        execution_path,
        &config,
        &plan,
        batch_size,
        include_row_counts,
        resolve_compat(req.export_compat.as_deref()),
    )
    .await;

    match result {
        Ok(rows_exported) => {
            let duration_ms = start.elapsed().as_millis();
            let summary = build_summary(
                ExportWorkload::Data,
                execution_path,
                duration_ms,
                Some(rows_exported),
                warnings,
            );
            tracing::info!(
                workload = "data",
                ?source_db_type,
                ?target_dialect,
                ?execution_path,
                rows_exported,
                duration_ms,
                output_path = %plan.output_path.display(),
                "Export completed"
            );
            response::ok(ExportResponse {
                success: true,
                message: "Data exported successfully".to_string(),
                file_path: Some(plan.output_path.to_string_lossy().to_string()),
                summary: Some(summary),
            })
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to export data");
            response::err_with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to export data",
                ErrorCode::ExportFailed,
            )
        }
    }
}

pub async fn get_export_directory() -> ApiResult<String> {
    match exports_dir() {
        Ok(dir) => response::ok(dir.to_string_lossy().to_string()),
        Err(e) => response::err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::{export_data, export_ddl};
    use crate::models::{ConnectionConfig, DbType, ExportRequest};
    use axum::{extract::Json, http::StatusCode};

    fn sample_request() -> ExportRequest {
        ExportRequest {
            config: ConnectionConfig {
                db_type: DbType::Dm8,
                host: "localhost".to_string(),
                port: 5236,
                username: "SYSDBA".to_string(),
                password: "SYSDBA".to_string(),
                schema: "SYSDBA".to_string(),
                export_schema: None,
            },
            target_dialect: None,
            export_schema: None,
            export_compat: None,
            tables: vec![],
            include_ddl: true,
            include_data: false,
            batch_size: Some(1000),
            drop_existing: true,
            include_row_counts: false,
            strict_mode: false,
        }
    }

    #[tokio::test]
    async fn export_ddl_succeeds_for_kingbase_to_mysql() {
        let mut req = sample_request();
        req.config.db_type = DbType::Kingbase;
        req.target_dialect = Some(DbType::Mysql);

        let out = export_ddl(Json(req)).await;
        // This path is now implemented, but will fail due to missing connection
        // We expect INTERNAL_SERVER_ERROR (500) not BAD_REQUEST (400)
        let err = out.expect_err("should fail due to connection, not unsupported path");
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn export_data_returns_unprocessable_entity_in_strict_partial_mode() {
        let mut req = sample_request();
        req.config.db_type = DbType::Mysql;
        req.target_dialect = Some(DbType::Mysql);
        req.include_ddl = false;
        req.include_data = true;
        req.strict_mode = true;

        let out = export_data(Json(req)).await;
        let err = out.expect_err("strict mode should reject partial capability");
        assert_eq!(err.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!err.1 .0.success, "error response must set success=false");
        assert!(
            err.1
                 .0
                .error
                .unwrap_or_default()
                .contains("Strict mode rejection"),
            "strict mode error should be surfaced"
        );
    }
}
