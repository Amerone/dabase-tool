use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    Env,
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
    pub export_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConnectionResponse {
    pub config: ConnectionConfig,
    pub source: ConfigSource,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportRequest {
    pub config: ConnectionConfig,
    pub export_schema: Option<String>,
    pub export_compat: Option<String>,
    pub tables: Vec<String>,
    pub include_ddl: bool,
    pub include_data: bool,
    pub batch_size: Option<usize>,
    #[serde(default = "default_true")]
    pub drop_existing: bool,
    #[serde(default = "default_false")]
    pub include_row_counts: bool,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}
