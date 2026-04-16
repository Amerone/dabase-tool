use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    Env,
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbType {
    #[default]
    Dm8,
    Mysql,
    Kingbase,
    Shentong,
}

impl DbType {
    pub fn filename_label(&self) -> &'static str {
        match self {
            DbType::Dm8 => "dm8",
            DbType::Mysql => "mysql",
            DbType::Kingbase => "kingbase",
            DbType::Shentong => "shentong",
        }
    }
}

fn default_db_type() -> DbType {
    DbType::Dm8
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionConfig {
    #[serde(default = "default_db_type")]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
    pub export_schema: Option<String>,
    pub database: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConnectionResponse {
    pub config: ConnectionConfig,
    pub source: ConfigSource,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedConnectionResponse {
    pub id: i64,
    pub name: String,
    pub config: ConnectionConfig,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub schema: Option<String>,
    pub comment: Option<String>,
    pub row_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub length: Option<i32>,
    pub precision: Option<i32>,
    pub scale: Option<i32>,
    pub char_semantics: Option<String>,
    pub nullable: bool,
    pub comment: Option<String>,
    pub default_value: Option<String>,
    #[serde(default)]
    pub identity: bool,
    pub identity_start: Option<i64>,
    pub identity_increment: Option<i64>,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniqueConstraint {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConstraint {
    pub name: String,
    pub condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub delete_rule: Option<String>,
    pub update_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDetails {
    pub name: String,
    pub comment: Option<String>,
    pub columns: Vec<Column>,
    pub primary_keys: Vec<String>,
    pub indexes: Vec<Index>,
    pub unique_constraints: Vec<UniqueConstraint>,
    pub foreign_keys: Vec<ForeignKey>,
    pub check_constraints: Vec<CheckConstraint>,
    pub triggers: Vec<TriggerDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableIdentifier {
    pub schema: String,
    pub name: String,
}

impl std::fmt::Display for TableIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.schema, self.name)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportRequest {
    pub config: ConnectionConfig,
    pub target_dialect: Option<DbType>,
    pub export_schema: Option<String>,
    pub export_compat: Option<String>,
    pub tables: Vec<TableIdentifier>,
    pub include_ddl: bool,
    pub include_data: bool,
    pub batch_size: Option<usize>,
    #[serde(default = "default_true")]
    pub drop_existing: bool,
    #[serde(default = "default_false")]
    pub include_row_counts: bool,
    #[serde(default = "default_false")]
    pub strict_mode: bool,
    pub identifier_case: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExportObjectKind {
    Ddl,
    Data,
    Columns,
    PrimaryKeys,
    Indexes,
    UniqueConstraints,
    ForeignKeys,
    CheckConstraints,
    Triggers,
    Sequences,
}

impl ExportObjectKind {
    pub const ALL: [ExportObjectKind; 10] = [
        ExportObjectKind::Ddl,
        ExportObjectKind::Data,
        ExportObjectKind::Columns,
        ExportObjectKind::PrimaryKeys,
        ExportObjectKind::Indexes,
        ExportObjectKind::UniqueConstraints,
        ExportObjectKind::ForeignKeys,
        ExportObjectKind::CheckConstraints,
        ExportObjectKind::Triggers,
        ExportObjectKind::Sequences,
    ];
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityLevel {
    None,
    Partial,
    Full,
}

impl CapabilityLevel {
    pub fn effective_with(self, other: CapabilityLevel) -> CapabilityLevel {
        self.min(other)
    }

    pub fn is_supported(self) -> bool {
        self != CapabilityLevel::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectCapability {
    pub object: ExportObjectKind,
    pub level: CapabilityLevel,
    pub note: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilityProfile {
    pub items: Vec<ObjectCapability>,
}

impl CapabilityProfile {
    pub fn level_for(&self, object: ExportObjectKind) -> CapabilityLevel {
        self.items
            .iter()
            .find(|item| item.object == object)
            .map(|item| item.level)
            .unwrap_or(CapabilityLevel::None)
    }

    pub fn note_for(&self, object: ExportObjectKind) -> Option<&str> {
        self.items
            .iter()
            .find(|item| item.object == object)
            .and_then(|item| item.note.as_deref())
    }

    pub fn reason_code_for(&self, object: ExportObjectKind) -> Option<&str> {
        self.items
            .iter()
            .find(|item| item.object == object)
            .and_then(|item| item.reason_code.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportCapabilityEntry {
    pub object: ExportObjectKind,
    pub source_level: CapabilityLevel,
    pub target_level: CapabilityLevel,
    pub effective_level: CapabilityLevel,
    pub note: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportCapabilityReport {
    pub source_db_type: DbType,
    pub target_dialect: DbType,
    pub entries: Vec<ExportCapabilityEntry>,
    #[serde(default)]
    pub data_options: ExportDataOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportDataOptions {
    pub include_row_counts_supported: bool,
    pub include_row_counts_note: Option<String>,
}

impl Default for ExportDataOptions {
    fn default() -> Self {
        Self {
            include_row_counts_supported: false,
            include_row_counts_note: Some("行数预扫描能力待执行路径解析后确定".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub name: String,
    pub min_value: Option<i64>,
    pub max_value: Option<i64>,
    pub increment_by: i64,
    pub cache_size: Option<i64>,
    pub cycle: bool,
    pub order: bool,
    pub start_with: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDefinition {
    pub name: String,
    pub table_name: String,
    pub timing: String,
    pub events: Vec<String>,
    pub each_row: bool,
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportResponse {
    pub success: bool,
    pub message: String,
    pub file_path: Option<String>,
    pub summary: Option<ExportExecutionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportExecutionSummary {
    pub workload: ExportObjectKind,
    pub execution_path: String,
    pub duration_ms: u128,
    pub rows_exported: Option<usize>,
    pub warnings: Vec<String>,
    pub skipped_objects: Vec<ExportSkippedObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSkippedObject {
    pub object_kind: ExportObjectKind,
    pub object_name: String,
    pub reason_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    DatabaseConnection,
    DatabaseQuery,
    InvalidConfig,
    ExportFailed,
    FileIo,
    Internal,
    NotSupported,
    ValidationFailed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            error_code: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
            error_code: None,
        }
    }

    pub fn error_with_code(message: String, code: ErrorCode) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
            error_code: Some(code),
        }
    }
}

impl ConnectionConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.host.trim().is_empty(), "Database host is required");
        anyhow::ensure!(self.port > 0, "Database port must be greater than zero");
        anyhow::ensure!(
            !self.username.trim().is_empty(),
            "Database username is required"
        );
        anyhow::ensure!(!self.password.is_empty(), "Database password is required");
        anyhow::ensure!(
            !self.schema.trim().is_empty(),
            "Database schema is required"
        );
        Ok(())
    }
}
