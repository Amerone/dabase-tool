pub mod mysql_renderer;

use anyhow::{anyhow, Result};

use crate::domain::canonical::{CanonicalRow, CanonicalTable, CanonicalValue, LogicalType};
use crate::models::{
    CapabilityLevel, CapabilityProfile, DbType, ExportObjectKind, ObjectCapability,
};

pub trait DialectRenderer: Send + Sync {
    fn dialect_kind(&self) -> DbType;
    fn capabilities(&self) -> CapabilityProfile;

    fn render_table_ddl(&self, _table: &CanonicalTable) -> Result<String> {
        Err(anyhow!(
            "Dialect {:?} table DDL rendering is not implemented",
            self.dialect_kind()
        ))
    }

    fn render_insert_batch(
        &self,
        _table: &CanonicalTable,
        _rows: &[CanonicalRow],
    ) -> Result<String> {
        Err(anyhow!(
            "Dialect {:?} insert rendering is not implemented",
            self.dialect_kind()
        ))
    }
}

#[derive(Debug, Default)]
pub struct Dm8DialectRenderer;

#[derive(Debug, Default)]
pub struct KingbaseDialectRenderer;

#[derive(Debug, Default)]
pub struct ShentongDialectRenderer;

impl DialectRenderer for Dm8DialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Dm8
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "DM8 方言渲染器已在旧版导出模块中实现",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "DM8 数据渲染器已在旧版导出模块中实现",
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

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                dm8_quote_ident(&col.name),
                dm8_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| dm8_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            dm8_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let columns = table
            .columns
            .iter()
            .map(|col| dm8_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        let mut statements = Vec::with_capacity(rows.len());
        for row in rows {
            let values = row
                .values
                .iter()
                .map(dm8_format_value)
                .collect::<Vec<_>>()
                .join(", ");
            statements.push(format!(
                "INSERT INTO {} ({}) VALUES ({});",
                dm8_quote_ident(&table.name),
                columns,
                values
            ));
        }

        Ok(statements.join("\n"))
    }
}

impl DialectRenderer for KingbaseDialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Kingbase
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "KingbaseES 渲染器支持完整 DDL 生成（表/列/主键/索引/约束/触发器/序列）",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "KingbaseES 渲染器支持 INSERT 批量生成",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Full,
                "支持 CREATE INDEX",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT UNIQUE",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT FOREIGN KEY",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT CHECK",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::Full,
                "支持 CREATE TRIGGER (PostgreSQL 语法)",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::Full,
                "支持 CREATE SEQUENCE (PostgreSQL 语法)",
            ),
        ])
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                kingbase_quote_ident(&col.name),
                kingbase_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| kingbase_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            kingbase_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let columns = table
            .columns
            .iter()
            .map(|col| kingbase_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        let values = rows
            .iter()
            .map(|row| {
                let literals = row
                    .values
                    .iter()
                    .map(kingbase_format_value)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", literals)
            })
            .collect::<Vec<_>>()
            .join(",\n");

        Ok(format!(
            "INSERT INTO {} ({}) VALUES\n{};",
            kingbase_quote_ident(&table.name),
            columns,
            values
        ))
    }
}

impl DialectRenderer for ShentongDialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Shentong
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "神通 OSCAR 渲染器支持完整 DDL 生成（Oracle 兼容语法）",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "神通 OSCAR 渲染器支持 INSERT 语句批量生成",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Partial,
                "基础索引支持（通过 information_schema）",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Partial,
                "唯一约束基础支持",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Partial,
                "外键基础支持",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::None,
                "神通 OSCAR 检查约束元数据暂不支持",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::None,
                "神通 OSCAR 触发器元数据暂不支持",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::None,
                "神通 OSCAR 序列元数据暂不支持",
            ),
        ])
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                shentong_quote_ident(&col.name),
                shentong_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| shentong_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            shentong_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let columns = table
            .columns
            .iter()
            .map(|col| shentong_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        let mut statements = Vec::with_capacity(rows.len());
        for row in rows {
            let values = row
                .values
                .iter()
                .map(shentong_format_value)
                .collect::<Vec<_>>()
                .join(", ");
            statements.push(format!(
                "INSERT INTO {} ({}) VALUES ({});",
                shentong_quote_ident(&table.name),
                columns,
                values
            ));
        }

        Ok(statements.join("\n"))
    }
}

pub fn renderer_for(db_type: &DbType) -> Box<dyn DialectRenderer> {
    match db_type {
        DbType::Dm8 => Box::new(Dm8DialectRenderer),
        DbType::Mysql => Box::new(mysql_renderer::MySqlDialectRenderer),
        DbType::Kingbase => Box::new(KingbaseDialectRenderer),
        DbType::Shentong => Box::new(ShentongDialectRenderer),
    }
}

fn shentong_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn shentong_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "INTEGER",
        LogicalType::Decimal => "DECIMAL(38,10)",
        LogicalType::Float => "DOUBLE PRECISION",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "CLOB",
        LogicalType::Binary => "BLOB",
        LogicalType::Boolean => "CHAR(1)",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "CLOB",
        LogicalType::Unknown => "CLOB",
    }
}

fn shentong_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "'1'".to_string()
            } else {
                "'0'".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            let hex = v.iter().map(|b| format!("{:02X}", b)).collect::<String>();
            format!("HEXTORAW('{}')", hex)
        }
    }
}

fn kingbase_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn kingbase_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "BIGINT",
        LogicalType::Decimal => "NUMERIC(38,10)",
        LogicalType::Float => "DOUBLE PRECISION",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "TEXT",
        LogicalType::Binary => "BYTEA",
        LogicalType::Boolean => "BOOLEAN",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "JSONB",
        LogicalType::Unknown => "TEXT",
    }
}

fn kingbase_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            let hex = v.iter().map(|b| format!("{:02X}", b)).collect::<String>();
            format!("decode('{}', 'hex')", hex)
        }
    }
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

fn dm8_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn dm8_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "BIGINT",
        LogicalType::Decimal => "DECIMAL(38,10)",
        LogicalType::Float => "DOUBLE",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "CLOB",
        LogicalType::Binary => "BLOB",
        LogicalType::Boolean => "NUMBER(1)",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "CLOB",
        LogicalType::Unknown => "CLOB",
    }
}

fn dm8_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            let hex = v.iter().map(|b| format!("{:02X}", b)).collect::<String>();
            format!("HEXTORAW('{}')", hex)
        }
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

#[cfg(test)]
mod tests {
    use super::{DialectRenderer, Dm8DialectRenderer};
    use crate::domain::canonical::{CanonicalColumn, CanonicalTable, LogicalType};

    #[test]
    fn dm8_renderer_quotes_schema_qualified_name() {
        let renderer = Dm8DialectRenderer;
        let table = CanonicalTable {
            name: "APP.USERS".to_string(),
            columns: vec![CanonicalColumn {
                name: "ID".to_string(),
                logical_type: LogicalType::Integer,
                nullable: false,
            }],
            primary_keys: vec!["ID".to_string()],
        };

        let ddl = renderer
            .render_table_ddl(&table)
            .expect("schema-qualified DM8 table should render");
        assert!(ddl.contains("CREATE TABLE \"APP\".\"USERS\""));
    }
}
