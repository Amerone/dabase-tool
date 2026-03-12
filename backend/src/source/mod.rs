use crate::domain::canonical::{canonical_table_from_details, CanonicalTable};
use crate::models::{
    CapabilityLevel, CapabilityProfile, DbType, ExportObjectKind, ObjectCapability, TableDetails,
};

pub trait SourceAdapter: Send + Sync {
    fn source_kind(&self) -> DbType;
    fn capabilities(&self) -> CapabilityProfile;
}

#[derive(Debug, Default)]
pub struct Dm8SourceAdapter;

#[derive(Debug, Default)]
pub struct MysqlSourceAdapter;

#[derive(Debug, Default)]
pub struct KingbaseSourceAdapter;

#[derive(Debug, Default)]
pub struct ShentongSourceAdapter;

impl SourceAdapter for Dm8SourceAdapter {
    fn source_kind(&self) -> DbType {
        DbType::Dm8
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "DM8 源端元数据支持完整 DDL 导出",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "DM8 源端支持数据提取导出",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (ExportObjectKind::Indexes, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "",
            ),
            (ExportObjectKind::ForeignKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Full,
                "",
            ),
            (ExportObjectKind::Triggers, CapabilityLevel::Full, ""),
            (ExportObjectKind::Sequences, CapabilityLevel::Full, ""),
        ])
    }
}

impl SourceAdapter for MysqlSourceAdapter {
    fn source_kind(&self) -> DbType {
        DbType::Mysql
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Partial,
                "MySQL 源端暴露核心 DDL 元数据，渲染器侧覆盖仍不完整",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Partial,
                "MySQL 源端可提供行数据，高级类型映射待完善",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Full,
                "MySQL 源端适配器通过 information_schema 支持索引元数据",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "MySQL source adapter supports unique constraint metadata",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Full,
                "MySQL source adapter supports foreign key metadata",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Partial,
                "Check constraint metadata depends on MySQL version and privileges",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::Full,
                "MySQL source adapter supports trigger metadata",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::None,
                "MySQL source does not provide sequence metadata in current adapter",
            ),
        ])
    }
}

impl SourceAdapter for KingbaseSourceAdapter {
    fn source_kind(&self) -> DbType {
        DbType::Kingbase
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "KingbaseES 源端支持完整元数据提取（通过 pg_catalog）",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "KingbaseES 源端支持数据提取",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Full,
                "通过 pg_indexes 提取",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "通过 pg_constraint 提取",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Full,
                "通过 pg_constraint 提取",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Full,
                "通过 pg_constraint 提取",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::Partial,
                "通过 pg_trigger 提取",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::Full,
                "通过 pg_class (relkind='S') 提取",
            ),
        ])
    }
}

impl SourceAdapter for ShentongSourceAdapter {
    fn source_kind(&self) -> DbType {
        DbType::Shentong
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Partial,
                "神通 OSCAR 源端通过 information_schema 提取基础 DDL 元数据",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Partial,
                "神通 OSCAR 源端可通过 ODBC 流式读取行数据，高级类型映射待完善",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::None,
                "神通 OSCAR 索引元数据提取待完成",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::None,
                "神通 OSCAR 唯一约束元数据提取待完成",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::None,
                "神通 OSCAR 外键元数据提取待完成",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::None,
                "神通 OSCAR 检查约束元数据提取待完成",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::None,
                "神通 OSCAR 触发器元数据提取待完成",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::None,
                "神通 OSCAR 序列元数据提取待完成",
            ),
        ])
    }
}

pub fn adapter_for(db_type: &DbType) -> Box<dyn SourceAdapter> {
    match db_type {
        DbType::Dm8 => Box::new(Dm8SourceAdapter),
        DbType::Mysql => Box::new(MysqlSourceAdapter),
        DbType::Kingbase => Box::new(KingbaseSourceAdapter),
        DbType::Shentong => Box::new(ShentongSourceAdapter),
    }
}

pub fn to_canonical_table(details: &TableDetails) -> CanonicalTable {
    canonical_table_from_details(details)
}

fn capability_profile(defs: &[(ExportObjectKind, CapabilityLevel, &str)]) -> CapabilityProfile {
    CapabilityProfile {
        items: defs
            .iter()
            .map(|(object, level, note)| ObjectCapability {
                object: *object,
                level: *level,
                note: non_empty_note(note),
                reason_code: reason_code_for_level(*level),
            })
            .collect(),
    }
}

fn reason_code_for_level(level: CapabilityLevel) -> Option<String> {
    match level {
        CapabilityLevel::Full => None,
        CapabilityLevel::Partial => Some("partial_support".to_string()),
        CapabilityLevel::None => Some("not_supported".to_string()),
    }
}

fn non_empty_note(note: &str) -> Option<String> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
