use crate::{
    dialect::renderer_for,
    export::orchestrator::{ExportExecutionPath, ExportWorkload, LegacyExportOrchestrator},
    models::{
        CapabilityLevel, DbType, ExportCapabilityEntry, ExportCapabilityReport, ExportDataOptions,
        ExportObjectKind,
    },
    source::adapter_for,
};

pub fn build_export_capability_report(
    source_db_type: DbType,
    target_dialect: DbType,
) -> ExportCapabilityReport {
    let source = adapter_for(&source_db_type);
    let dialect = renderer_for(&target_dialect);
    let source_caps = source.capabilities();
    let target_caps = dialect.capabilities();

    let entries = ExportObjectKind::ALL
        .iter()
        .map(|object| {
            let source_level = source_caps.level_for(*object);
            let target_level = target_caps.level_for(*object);
            let effective_level = source_level.effective_with(target_level);
            let note = join_notes(source_caps.note_for(*object), target_caps.note_for(*object));
            let reason_code = join_reasons(
                source_caps.reason_code_for(*object),
                target_caps.reason_code_for(*object),
            );

            ExportCapabilityEntry {
                object: *object,
                source_level,
                target_level,
                effective_level,
                note,
                reason_code,
            }
        })
        .collect();
    let data_options = data_options_for_pair(&source_db_type, &target_dialect);

    ExportCapabilityReport {
        source_db_type,
        target_dialect,
        entries,
        data_options,
    }
}

pub fn build_runtime_export_capability_report(
    source_db_type: DbType,
    target_dialect: DbType,
) -> ExportCapabilityReport {
    let mut report = build_export_capability_report(source_db_type.clone(), target_dialect.clone());

    let ddl_execution_result = LegacyExportOrchestrator::resolve_execution_path(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Ddl,
    );
    let data_execution_result = LegacyExportOrchestrator::resolve_execution_path(
        &source_db_type,
        &target_dialect,
        ExportWorkload::Data,
    );

    apply_execution_path_constraint(&mut report, ExportObjectKind::Ddl, ddl_execution_result);
    apply_execution_path_constraint(
        &mut report,
        ExportObjectKind::Data,
        data_execution_result.clone(),
    );
    apply_data_option_constraint(&mut report, data_execution_result);

    report
}

fn data_options_for_pair(source_db_type: &DbType, target_dialect: &DbType) -> ExportDataOptions {
    let supported = matches!((source_db_type, target_dialect), (DbType::Dm8, DbType::Dm8));
    let note = if supported {
        Some("DM8 旧版导出路径支持行数预扫描".to_string())
    } else {
        Some("当前源/目标组合不支持行数预扫描".to_string())
    };

    ExportDataOptions {
        include_row_counts_supported: supported,
        include_row_counts_note: note,
    }
}

fn apply_execution_path_constraint(
    report: &mut ExportCapabilityReport,
    object: ExportObjectKind,
    execution_result: Result<ExportExecutionPath, String>,
) {
    let Some(entry) = report
        .entries
        .iter_mut()
        .find(|entry| entry.object == object)
    else {
        return;
    };

    if let Err(message) = execution_result {
        entry.effective_level = CapabilityLevel::None;
        entry.reason_code = Some("execution_path_missing".to_string());
        entry.note = match entry.note.take() {
            Some(existing) => Some(format!("{}; {}", existing, message)),
            None => Some(message),
        };
    }
}

fn apply_data_option_constraint(
    report: &mut ExportCapabilityReport,
    data_execution_result: Result<ExportExecutionPath, String>,
) {
    match data_execution_result {
        Ok(path) => {
            if LegacyExportOrchestrator::supports_include_row_counts(path) {
                report.data_options.include_row_counts_supported = true;
                report.data_options.include_row_counts_note =
                    Some("当前执行路径支持行数预扫描".to_string());
            } else {
                report.data_options.include_row_counts_supported = false;
                report.data_options.include_row_counts_note =
                    Some("当前执行路径不支持行数预扫描".to_string());
            }
        }
        Err(message) => {
            report.data_options.include_row_counts_supported = false;
            report.data_options.include_row_counts_note = Some(format!(
                "行数预扫描不可用，数据导出路径无法解析: {}",
                message
            ));
        }
    }
}

fn join_notes(source: Option<&str>, target: Option<&str>) -> Option<String> {
    match (source, target) {
        (Some(s), Some(t)) if s == t => Some(s.to_string()),
        (Some(s), Some(t)) => Some(format!("源端: {}; 目标端: {}", s, t)),
        (Some(s), None) => Some(format!("源端: {}", s)),
        (None, Some(t)) => Some(format!("目标端: {}", t)),
        (None, None) => None,
    }
}

fn join_reasons(source: Option<&str>, target: Option<&str>) -> Option<String> {
    match (source, target) {
        (Some(s), Some(t)) if s == t => Some(s.to_string()),
        (Some(s), Some(t)) => Some(format!("source:{}|target:{}", s, t)),
        (Some(s), None) => Some(format!("source:{}", s)),
        (None, Some(t)) => Some(format!("target:{}", t)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod reason_tests {
    #[test]
    fn join_reasons_prefers_shared_value() {
        let out = super::join_reasons(Some("same"), Some("same"));
        assert_eq!(out.as_deref(), Some("same"));
    }

    #[test]
    fn join_reasons_merges_source_and_target() {
        let out = super::join_reasons(Some("s"), Some("t"));
        assert_eq!(out.as_deref(), Some("source:s|target:t"));
    }
}

#[cfg(test)]
mod tests {
    use super::{build_export_capability_report, build_runtime_export_capability_report};
    use crate::models::{CapabilityLevel, DbType, ExportObjectKind};

    #[test]
    fn dm8_to_dm8_has_full_ddl_and_data() {
        let report = build_export_capability_report(DbType::Dm8, DbType::Dm8);
        let ddl = report
            .entries
            .iter()
            .find(|entry| entry.object == ExportObjectKind::Ddl)
            .expect("missing DDL capability");
        assert_eq!(ddl.effective_level, CapabilityLevel::Full);

        let data = report
            .entries
            .iter()
            .find(|entry| entry.object == ExportObjectKind::Data)
            .expect("missing DATA capability");
        assert_eq!(data.effective_level, CapabilityLevel::Full);
    }

    #[test]
    fn mysql_to_mysql_has_partial_support() {
        let report = build_export_capability_report(DbType::Mysql, DbType::Mysql);
        let ddl = report
            .entries
            .iter()
            .find(|entry| entry.object == ExportObjectKind::Ddl)
            .expect("missing DDL capability");
        assert_eq!(ddl.effective_level, CapabilityLevel::Partial);
    }

    #[test]
    fn runtime_report_reflects_implemented_path() {
        let report = build_runtime_export_capability_report(DbType::Kingbase, DbType::Mysql);
        let data = report
            .entries
            .iter()
            .find(|entry| entry.object == ExportObjectKind::Data)
            .expect("missing DATA capability");
        // KingBase->MySQL is now implemented, so it should be Partial (based on source/target capabilities)
        assert_eq!(data.effective_level, CapabilityLevel::Partial);
    }
}
