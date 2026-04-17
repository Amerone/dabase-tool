use std::path::Path;

use anyhow::Result;
use odbc_api::Connection;

use crate::{
    export::{
        capability::build_export_capability_report,
        data::{export_schema_data, SchemaDataExportSpec},
        ddl::{export_schema_ddl, TriggerTerminator},
    },
    models::{CapabilityLevel, DbType, ExportObjectKind, TableIdentifier},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportWorkload {
    Ddl,
    Data,
}

impl ExportWorkload {
    fn as_object(self) -> ExportObjectKind {
        match self {
            ExportWorkload::Ddl => ExportObjectKind::Ddl,
            ExportWorkload::Data => ExportObjectKind::Data,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportExecutionPath {
    LegacyDm8,
    Dm8ToMysqlPoc,
    Dm8ToKingbasePoc,
    KingbasePoc,
    KingbaseToMysqlPoc,
    KingbaseToDm8Poc,
    MysqlPoc,
    MysqlToDm8Poc,
    MysqlToKingbasePoc,
    ShentongPoc,
    Dm8ToShentongPoc,
}

#[derive(Debug, Clone)]
pub struct LegacyExportPlan {
    pub source_schema: String,
    pub target_schema: String,
    pub tables: Vec<TableIdentifier>,
    pub output_path: std::path::PathBuf,
}

pub struct LegacyExportOrchestrator;

impl LegacyExportOrchestrator {
    pub fn resolve_execution_path(
        source_db_type: &DbType,
        target_dialect: &DbType,
        workload: ExportWorkload,
    ) -> Result<ExportExecutionPath, String> {
        let report = build_export_capability_report(source_db_type.clone(), target_dialect.clone());
        let entry = report
            .entries
            .iter()
            .find(|item| item.object == workload.as_object())
            .ok_or_else(|| "无法解析能力条目".to_string())?;

        if entry.effective_level == CapabilityLevel::None {
            let mut msg = format!(
                "不支持从 {:?} 导出 {:?} 到 {:?}",
                workload, source_db_type, target_dialect
            );
            if let Some(note) = &entry.note {
                msg.push_str(&format!(". {}", note));
            }
            return Err(msg);
        }

        match (source_db_type, target_dialect) {
            (DbType::Dm8, DbType::Dm8) => Ok(ExportExecutionPath::LegacyDm8),
            (DbType::Dm8, DbType::Mysql) => Ok(ExportExecutionPath::Dm8ToMysqlPoc),
            (DbType::Dm8, DbType::Kingbase) => Ok(ExportExecutionPath::Dm8ToKingbasePoc),
            (DbType::Kingbase, DbType::Kingbase) => Ok(ExportExecutionPath::KingbasePoc),
            (DbType::Kingbase, DbType::Mysql) => Ok(ExportExecutionPath::KingbaseToMysqlPoc),
            (DbType::Kingbase, DbType::Dm8) => Ok(ExportExecutionPath::KingbaseToDm8Poc),
            (DbType::Mysql, DbType::Mysql) => Ok(ExportExecutionPath::MysqlPoc),
            (DbType::Mysql, DbType::Dm8) => Ok(ExportExecutionPath::MysqlToDm8Poc),
            (DbType::Mysql, DbType::Kingbase) => Ok(ExportExecutionPath::MysqlToKingbasePoc),
            (DbType::Shentong, DbType::Shentong) => Ok(ExportExecutionPath::ShentongPoc),
            (DbType::Dm8, DbType::Shentong) => Ok(ExportExecutionPath::Dm8ToShentongPoc),
            _ => Err(format!(
                "源 {:?} → 目标 {:?} 的导出路径尚未实现",
                source_db_type, target_dialect
            )),
        }
    }

    pub fn export_ddl(
        connection: &Connection<'_>,
        plan: &LegacyExportPlan,
        drop_existing: bool,
        trigger_terminator: TriggerTerminator,
    ) -> Result<()> {
        export_schema_ddl(
            connection,
            &plan.source_schema,
            &plan.target_schema,
            &plan.tables,
            Path::new(&plan.output_path),
            drop_existing,
            trigger_terminator,
        )
    }

    pub fn export_data(
        connection: &Connection<'_>,
        plan: &LegacyExportPlan,
        batch_size: usize,
        include_row_counts: bool,
        compat: crate::export::ddl::TriggerTerminator,
    ) -> Result<usize> {
        export_schema_data(
            connection,
            SchemaDataExportSpec {
                source_schema: &plan.source_schema,
                target_schema: &plan.target_schema,
                tables: &plan.tables,
                output_path: Path::new(&plan.output_path),
                batch_size,
                include_row_counts,
                compat,
            },
        )
    }

    pub fn supports_include_row_counts(execution_path: ExportExecutionPath) -> bool {
        matches!(execution_path, ExportExecutionPath::LegacyDm8)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExportExecutionPath, ExportWorkload, LegacyExportOrchestrator};
    use crate::models::DbType;

    #[test]
    fn dm8_resolves_to_legacy_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Dm8,
            &DbType::Dm8,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::LegacyDm8);
    }

    #[test]
    fn mysql_resolves_to_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Mysql,
            &DbType::Mysql,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::MysqlPoc);
    }

    #[test]
    fn kingbase_resolves_to_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Kingbase,
            &DbType::Kingbase,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::KingbasePoc);
    }

    #[test]
    fn kingbase_to_mysql_now_implemented() {
        let data = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Kingbase,
            &DbType::Mysql,
            ExportWorkload::Data,
        );
        assert!(data.is_ok(), "Kingbase->MySQL is now implemented");
        assert_eq!(data.unwrap(), ExportExecutionPath::KingbaseToMysqlPoc);
    }

    #[test]
    fn dm8_to_mysql_ddl_uses_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Dm8,
            &DbType::Mysql,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::Dm8ToMysqlPoc);
    }

    #[test]
    fn dm8_to_mysql_data_uses_poc_path() {
        let data = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Dm8,
            &DbType::Mysql,
            ExportWorkload::Data,
        )
        .unwrap();
        assert_eq!(data, ExportExecutionPath::Dm8ToMysqlPoc);
    }

    #[test]
    fn mysql_to_dm8_resolves_to_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Mysql,
            &DbType::Dm8,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::MysqlToDm8Poc);

        let data = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Mysql,
            &DbType::Dm8,
            ExportWorkload::Data,
        )
        .unwrap();
        assert_eq!(data, ExportExecutionPath::MysqlToDm8Poc);
    }

    #[test]
    fn include_row_counts_only_supported_on_legacy_dm8_path() {
        assert!(LegacyExportOrchestrator::supports_include_row_counts(
            ExportExecutionPath::LegacyDm8
        ));
        assert!(!LegacyExportOrchestrator::supports_include_row_counts(
            ExportExecutionPath::MysqlPoc
        ));
    }

    #[test]
    fn shentong_resolves_to_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Shentong,
            &DbType::Shentong,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::ShentongPoc);

        let data = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Shentong,
            &DbType::Shentong,
            ExportWorkload::Data,
        )
        .unwrap();
        assert_eq!(data, ExportExecutionPath::ShentongPoc);
    }

    #[test]
    fn dm8_to_shentong_resolves_to_poc_path() {
        let ddl = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Dm8,
            &DbType::Shentong,
            ExportWorkload::Ddl,
        )
        .unwrap();
        assert_eq!(ddl, ExportExecutionPath::Dm8ToShentongPoc);

        let data = LegacyExportOrchestrator::resolve_execution_path(
            &DbType::Dm8,
            &DbType::Shentong,
            ExportWorkload::Data,
        )
        .unwrap();
        assert_eq!(data, ExportExecutionPath::Dm8ToShentongPoc);
    }
}
