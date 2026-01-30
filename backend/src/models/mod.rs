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
    pub nullable: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDetails {
    pub name: String,
    pub comment: Option<String>,
    pub columns: Vec<Column>,
    pub primary_keys: Vec<String>,
    pub indexes: Vec<Index>,
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
    pub tables: Vec<String>,
    pub include_ddl: bool,
    pub include_data: bool,
    pub batch_size: Option<usize>,
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
