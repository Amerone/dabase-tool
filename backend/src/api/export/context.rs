use std::path::{Path, PathBuf};

use crate::{
    export::{
        capability::build_runtime_export_capability_report,
        ddl::TriggerTerminator,
        orchestrator::{ExportExecutionPath, ExportWorkload, LegacyExportOrchestrator},
    },
    models::{
        CapabilityLevel, ConnectionConfig, DbType, ExportExecutionSummary, ExportObjectKind,
        ExportRequest,
    },
};

fn normalize_schema_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

pub(super) fn resolve_target_schema(source: &str, export_schema: Option<&str>) -> String {
    normalize_schema_value(export_schema).unwrap_or_else(|| source.trim().to_string())
}

pub(super) fn resolve_compat(value: Option<&str>) -> TriggerTerminator {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(mode) if mode.eq_ignore_ascii_case("script") => TriggerTerminator::Script,
        Some(mode) if mode.eq_ignore_ascii_case("datagrip-script") => {
            TriggerTerminator::DataGripScript
        }
        Some(mode) if mode.eq_ignore_ascii_case("datagrip") => TriggerTerminator::DataGrip,
        _ => TriggerTerminator::DataGrip,
    }
}

pub(super) fn resolve_target_dialect(req: &ExportRequest) -> DbType {
    req.target_dialect
        .clone()
        .unwrap_or_else(|| req.config.db_type.clone())
}

fn workload_object(workload: ExportWorkload) -> ExportObjectKind {
    match workload {
        ExportWorkload::Ddl => ExportObjectKind::Ddl,
        ExportWorkload::Data => ExportObjectKind::Data,
    }
}

pub(super) fn collect_workload_warnings(
    source_db_type: &DbType,
    target_dialect: &DbType,
    workload: ExportWorkload,
    strict_mode: bool,
) -> Result<Vec<String>, String> {
    let report =
        build_runtime_export_capability_report(source_db_type.clone(), target_dialect.clone());
    let object = workload_object(workload);
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.object == object)
        .ok_or_else(|| "Failed to resolve capability entry".to_string())?;

    match entry.effective_level {
        CapabilityLevel::Full => Ok(Vec::new()),
        CapabilityLevel::Partial => {
            let mut warning = format!(
                "{:?} export runs with partial capability for source {:?} -> target {:?}",
                workload, source_db_type, target_dialect
            );
            if let Some(note) = &entry.note {
                warning.push_str(&format!(". {}", note));
            }
            if let Some(reason_code) = &entry.reason_code {
                warning.push_str(&format!(" [{}]", reason_code));
            }

            if strict_mode {
                return Err(format!("Strict mode rejection. {}", warning));
            }

            Ok(vec![warning])
        }
        CapabilityLevel::None => {
            let mut message = format!(
                "{:?} export is not supported for source {:?} -> target {:?}",
                workload, source_db_type, target_dialect
            );
            if let Some(note) = &entry.note {
                message.push_str(&format!(". {}", note));
            }
            if let Some(reason_code) = &entry.reason_code {
                message.push_str(&format!(" [{}]", reason_code));
            }
            Err(message)
        }
    }
}

pub(super) fn resolve_include_row_counts(
    execution_path: ExportExecutionPath,
    requested_include_row_counts: bool,
    strict_mode: bool,
    warnings: &mut Vec<String>,
) -> Result<bool, String> {
    if !requested_include_row_counts {
        return Ok(false);
    }

    if LegacyExportOrchestrator::supports_include_row_counts(execution_path) {
        return Ok(true);
    }

    let warning = format!(
        "include_row_counts is unsupported for execution path {:?}; option will be ignored",
        execution_path
    );
    if strict_mode {
        return Err(format!("Strict mode rejection. {}", warning));
    }
    warnings.push(warning);
    Ok(false)
}

/// Replace characters that are unsafe in file paths with underscores.
pub(super) fn sanitize_filename_part(s: &str) -> String {
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

pub(super) fn identifier_case_filename_marker(identifier_case: Option<&str>) -> Option<String> {
    let normalized = identifier_case
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_ascii_lowercase();

    match normalized.as_str() {
        "lower" | "upper" | "preserve" => Some(format!("case_{normalized}")),
        _ => None,
    }
}

pub(super) fn default_exports_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Unable to determine home directory".to_string())?;
    ensure_export_dir(home.join(".amarone").join("exports"))
}

pub(super) fn resolve_export_dir(directory: Option<&str>) -> Result<PathBuf, String> {
    let Some(raw) = directory.map(str::trim).filter(|value| !value.is_empty()) else {
        return default_exports_dir();
    };

    let dir = PathBuf::from(raw);
    if !dir.is_absolute() {
        return Err("Export directory must be an absolute path".to_string());
    }

    ensure_export_dir(dir)
}

fn ensure_export_dir(dir: PathBuf) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create export directory {:?}: {}", dir, e))?;
    Ok(dir)
}

#[cfg(test)]
pub(super) fn format_export_filename(
    source: &str,
    dialect: &str,
    target: &str,
    kind: &str,
    suffix: &str,
) -> Result<PathBuf, String> {
    let dir = default_exports_dir()?;
    format_export_filename_in_dir(&dir, source, dialect, target, kind, suffix, None)
}

pub(super) fn format_export_filename_in_dir(
    dir: &Path,
    source: &str,
    dialect: &str,
    target: &str,
    kind: &str,
    suffix: &str,
    identifier_case: Option<&str>,
) -> Result<PathBuf, String> {
    let case_marker = identifier_case_filename_marker(identifier_case);
    let mut parts = vec![
        sanitize_filename_part(source),
        "to".to_string(),
        sanitize_filename_part(dialect),
        sanitize_filename_part(target),
        kind.to_string(),
    ];
    if let Some(marker) = case_marker {
        parts.push(marker);
    }
    parts.push(suffix.to_string());

    Ok(dir.join(format!(
        "{}.sql",
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    )))
}

#[cfg(test)]
pub(super) fn format_error_chain(err: &anyhow::Error) -> String {
    format!("{:#}", err)
}

pub(super) fn build_summary(
    workload: ExportWorkload,
    execution_path: ExportExecutionPath,
    duration_ms: u128,
    rows_exported: Option<usize>,
    warnings: Vec<String>,
) -> ExportExecutionSummary {
    ExportExecutionSummary {
        workload: workload_object(workload),
        execution_path: format!("{:?}", execution_path),
        duration_ms,
        rows_exported,
        warnings,
        skipped_objects: Vec::new(),
    }
}

pub(super) fn build_export_context(req: &mut ExportRequest) -> (ConnectionConfig, String, String) {
    let config = ConnectionConfig {
        db_type: req.config.db_type.clone(),
        host: std::mem::take(&mut req.config.host),
        port: req.config.port,
        username: std::mem::take(&mut req.config.username),
        password: std::mem::take(&mut req.config.password),
        schema: req.config.schema.clone(),
        export_schema: req.config.export_schema.clone(),
        database: req.config.database.clone(),
    };

    let source_schema = config.schema.clone();
    let target_schema = resolve_target_schema(
        &source_schema,
        req.export_schema
            .as_deref()
            .or(config.export_schema.as_deref()),
    );

    (config, source_schema, target_schema)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_workload_warnings, format_error_chain, format_export_filename,
        format_export_filename_in_dir, identifier_case_filename_marker, resolve_compat,
        resolve_export_dir, resolve_include_row_counts, resolve_target_dialect,
        resolve_target_schema, sanitize_filename_part,
    };
    use crate::export::ddl::TriggerTerminator;
    use crate::export::orchestrator::{ExportExecutionPath, ExportWorkload};
    use crate::models::{ConnectionConfig, DbType, ExportRequest};

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
        let path =
            format_export_filename("SRC", "dm8", "TGT", "ddl", "20260130_120000_000").unwrap();
        assert!(path.is_absolute(), "export path must be absolute");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "SRC_to_dm8_TGT_ddl_20260130_120000_000.sql");
    }

    #[test]
    fn format_export_filename_sanitizes_special_chars() {
        let path = format_export_filename("../evil", "dm8", "a;b", "ddl", "ts").unwrap();
        assert!(path.is_absolute(), "export path must be absolute");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "___evil_to_dm8_a_b_ddl_ts.sql");
    }

    #[test]
    fn format_export_filename_uses_selected_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let path =
            format_export_filename_in_dir(dir.path(), "SRC", "dm8", "TGT", "ddl", "ts", None)
                .unwrap();
        assert_eq!(path.parent(), Some(dir.path()));
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "SRC_to_dm8_TGT_ddl_ts.sql"
        );
    }

    #[test]
    fn format_export_filename_includes_identifier_case_when_selected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = format_export_filename_in_dir(
            dir.path(),
            "SRC",
            "kingbase",
            "TGT",
            "ddl",
            "ts",
            Some("lower"),
        )
        .unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "SRC_to_kingbase_TGT_ddl_case_lower_ts.sql"
        );
    }

    #[test]
    fn identifier_case_filename_marker_normalizes_known_values() {
        assert_eq!(
            identifier_case_filename_marker(Some(" Upper ")).as_deref(),
            Some("case_upper")
        );
    }

    #[test]
    fn identifier_case_filename_marker_ignores_unknown_values() {
        assert_eq!(
            identifier_case_filename_marker(Some("mixed")).as_deref(),
            None
        );
    }

    #[test]
    fn resolve_export_dir_rejects_relative_path() {
        let err = resolve_export_dir(Some("relative/path")).unwrap_err();
        assert!(err.contains("absolute path"));
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
                database: None,
            },
            target_dialect: None,
            export_schema: None,
            export_directory: None,
            export_compat: None,
            tables: vec![],
            include_ddl: true,
            include_data: false,
            batch_size: Some(1000),
            drop_existing: true,
            include_row_counts: false,
            strict_mode: false,
            identifier_case: None,
        }
    }

    #[test]
    fn resolve_target_dialect_defaults_to_source_db_type() {
        let req = sample_request();
        assert_eq!(resolve_target_dialect(&req), DbType::Dm8);
    }

    #[test]
    fn resolve_target_dialect_uses_override_when_provided() {
        let mut req = sample_request();
        req.target_dialect = Some(DbType::Mysql);
        assert_eq!(resolve_target_dialect(&req), DbType::Mysql);
    }

    #[test]
    fn strict_mode_rejects_partial_capability() {
        let out =
            collect_workload_warnings(&DbType::Mysql, &DbType::Mysql, ExportWorkload::Ddl, true);
        assert!(out.is_err(), "strict mode should reject partial support");
    }

    #[test]
    fn non_strict_mode_keeps_partial_with_warning() {
        let out =
            collect_workload_warnings(&DbType::Mysql, &DbType::Mysql, ExportWorkload::Ddl, false)
                .unwrap();
        assert!(!out.is_empty(), "non-strict mode should emit warning");
    }

    #[test]
    fn strict_mode_accepts_full_capability() {
        let out = collect_workload_warnings(&DbType::Dm8, &DbType::Dm8, ExportWorkload::Ddl, true)
            .unwrap();
        assert!(out.is_empty(), "full capability should pass strict mode");
    }

    #[test]
    fn include_row_counts_is_forced_off_on_mysql_path_in_non_strict_mode() {
        let mut warnings = Vec::new();
        let resolved =
            resolve_include_row_counts(ExportExecutionPath::MysqlPoc, true, false, &mut warnings)
                .expect("non-strict mode should keep export running");
        assert!(!resolved);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn include_row_counts_rejected_in_strict_mode_when_path_unsupported() {
        let mut warnings = Vec::new();
        let resolved =
            resolve_include_row_counts(ExportExecutionPath::MysqlPoc, true, true, &mut warnings);
        assert!(resolved.is_err());
    }
}
