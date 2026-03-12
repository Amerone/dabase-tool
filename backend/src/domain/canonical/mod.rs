use serde::{Deserialize, Serialize};

use crate::models::{Column, TableDetails};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogicalType {
    Integer,
    Decimal,
    Float,
    String,
    Text,
    Binary,
    Boolean,
    Date,
    DateTime,
    Json,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalColumn {
    pub name: String,
    pub logical_type: LogicalType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalValue {
    Null,
    Integer(i64),
    Decimal(String),
    Float(f64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    Date(String),
    DateTime(String),
    Json(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CanonicalRow {
    pub values: Vec<CanonicalValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalTable {
    pub name: String,
    pub columns: Vec<CanonicalColumn>,
    pub primary_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CanonicalSchema {
    pub tables: Vec<CanonicalTable>,
}

pub fn logical_type_from_db_type(raw: &str) -> LogicalType {
    let upper = raw.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return LogicalType::Unknown;
    }

    if contains_any(
        &upper,
        &[
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "INTEGER",
            "BIGINT",
            "SERIAL",
        ],
    ) {
        return LogicalType::Integer;
    }
    if contains_any(&upper, &["DECIMAL", "NUMERIC"]) {
        return LogicalType::Decimal;
    }
    if contains_any(&upper, &["FLOAT", "DOUBLE", "REAL"]) {
        return LogicalType::Float;
    }
    if contains_any(&upper, &["CHAR", "VARCHAR", "NVARCHAR", "NCHAR"]) {
        return LogicalType::String;
    }
    if contains_any(&upper, &["TEXT", "CLOB"]) {
        return LogicalType::Text;
    }
    if contains_any(&upper, &["BLOB", "BINARY", "VARBINARY", "RAW", "BYTEA"]) {
        return LogicalType::Binary;
    }
    if contains_any(&upper, &["BOOL", "BOOLEAN", "BIT(1)"]) {
        return LogicalType::Boolean;
    }
    if contains_any(&upper, &["TIMESTAMP", "DATETIME"]) {
        return LogicalType::DateTime;
    }
    if contains_any(&upper, &["DATE"]) {
        return LogicalType::Date;
    }
    if contains_any(&upper, &["JSON"]) {
        return LogicalType::Json;
    }

    LogicalType::Unknown
}

pub fn canonical_column_from_model(column: &Column) -> CanonicalColumn {
    CanonicalColumn {
        name: column.name.clone(),
        logical_type: logical_type_from_db_type(&column.data_type),
        nullable: column.nullable,
    }
}

pub fn canonical_table_from_details(table: &TableDetails) -> CanonicalTable {
    CanonicalTable {
        name: table.name.clone(),
        columns: table
            .columns
            .iter()
            .map(canonical_column_from_model)
            .collect(),
        primary_keys: table.primary_keys.clone(),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{canonical_table_from_details, logical_type_from_db_type, LogicalType};
    use crate::models::{Column, TableDetails};

    fn sample_column(name: &str, data_type: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: data_type.to_string(),
            length: None,
            precision: None,
            scale: None,
            char_semantics: None,
            nullable: true,
            comment: None,
            default_value: None,
            identity: false,
            identity_start: None,
            identity_increment: None,
        }
    }

    #[test]
    fn logical_type_mapping_covers_common_types() {
        assert_eq!(logical_type_from_db_type("BIGINT"), LogicalType::Integer);
        assert_eq!(
            logical_type_from_db_type("decimal(10,2)"),
            LogicalType::Decimal
        );
        assert_eq!(logical_type_from_db_type("DOUBLE"), LogicalType::Float);
        assert_eq!(
            logical_type_from_db_type("VARCHAR(255)"),
            LogicalType::String
        );
        assert_eq!(logical_type_from_db_type("TEXT"), LogicalType::Text);
        assert_eq!(
            logical_type_from_db_type("VARBINARY(64)"),
            LogicalType::Binary
        );
        assert_eq!(logical_type_from_db_type("BOOLEAN"), LogicalType::Boolean);
        assert_eq!(logical_type_from_db_type("DATE"), LogicalType::Date);
        assert_eq!(
            logical_type_from_db_type("TIMESTAMP"),
            LogicalType::DateTime
        );
        assert_eq!(logical_type_from_db_type("JSON"), LogicalType::Json);
    }

    #[test]
    fn canonical_table_mapping_preserves_pk_and_columns() {
        let details = TableDetails {
            name: "users".to_string(),
            comment: None,
            columns: vec![
                sample_column("id", "BIGINT"),
                sample_column("name", "VARCHAR(255)"),
            ],
            primary_keys: vec!["id".to_string()],
            indexes: vec![],
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
            triggers: vec![],
        };

        let table = canonical_table_from_details(&details);
        assert_eq!(table.name, "users");
        assert_eq!(table.primary_keys, vec!["id"]);
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.columns[0].logical_type, LogicalType::Integer);
    }
}
