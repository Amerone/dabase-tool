use anyhow::Result;

use crate::{
    dialect::DialectRenderer,
    domain::canonical::{CanonicalRow, CanonicalTable, CanonicalValue, LogicalType},
    models::{CapabilityLevel, CapabilityProfile, DbType, ExportObjectKind, ObjectCapability},
};

#[derive(Debug, Default)]
pub struct MySqlDialectRenderer;

impl DialectRenderer for MySqlDialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Mysql
    }

    fn capabilities(&self) -> CapabilityProfile {
        CapabilityProfile {
            items: vec![
                ObjectCapability {
                    object: ExportObjectKind::Ddl,
                    level: CapabilityLevel::Partial,
                    note: Some("MySQL 渲染器当前支持表/列/主键 DDL".to_string()),
                    reason_code: Some("renderer_partial".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::Data,
                    level: CapabilityLevel::Partial,
                    note: Some("MySQL 渲染器当前支持基本 INSERT 批量生成".to_string()),
                    reason_code: Some("renderer_partial".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::Columns,
                    level: CapabilityLevel::Full,
                    note: None,
                    reason_code: None,
                },
                ObjectCapability {
                    object: ExportObjectKind::PrimaryKeys,
                    level: CapabilityLevel::Full,
                    note: None,
                    reason_code: None,
                },
                ObjectCapability {
                    object: ExportObjectKind::Indexes,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器索引渲染待完成".to_string()),
                    reason_code: Some("renderer_not_implemented".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::UniqueConstraints,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器唯一约束渲染待完成".to_string()),
                    reason_code: Some("renderer_not_implemented".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::ForeignKeys,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器外键渲染待完成".to_string()),
                    reason_code: Some("renderer_not_implemented".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::CheckConstraints,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器检查约束渲染待完成".to_string()),
                    reason_code: Some("renderer_not_implemented".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::Triggers,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器触发器渲染待完成".to_string()),
                    reason_code: Some("renderer_not_implemented".to_string()),
                },
                ObjectCapability {
                    object: ExportObjectKind::Sequences,
                    level: CapabilityLevel::None,
                    note: Some("MySQL 渲染器序列渲染不适用".to_string()),
                    reason_code: Some("not_applicable".to_string()),
                },
            ],
        }
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                quote_ident(&col.name),
                mysql_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        let columns = table
            .columns
            .iter()
            .map(|col| quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        let values = rows
            .iter()
            .map(|row| {
                let literals = row
                    .values
                    .iter()
                    .map(format_value)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", literals)
            })
            .collect::<Vec<_>>()
            .join(",\n");

        Ok(format!(
            "INSERT INTO {} ({}) VALUES\n{};",
            quote_ident(&table.name),
            columns,
            values
        ))
    }
}

fn quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("`{}`", part.replace('`', "``")))
        .collect::<Vec<_>>()
        .join(".")
}

fn mysql_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "BIGINT",
        LogicalType::Decimal => "DECIMAL(38,10)",
        LogicalType::Float => "DOUBLE",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "TEXT",
        LogicalType::Binary => "LONGBLOB",
        LogicalType::Boolean => "TINYINT(1)",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "DATETIME(6)",
        LogicalType::Json => "JSON",
        LogicalType::Unknown => "TEXT",
    }
}

fn format_value(value: &CanonicalValue) -> String {
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
        | CanonicalValue::Json(v) => format!("'{}'", escape_single_quotes(v)),
        CanonicalValue::Binary(v) => {
            let hex = v.iter().map(|b| format!("{:02X}", b)).collect::<String>();
            format!("X'{}'", hex)
        }
    }
}

fn escape_single_quotes(input: &str) -> String {
    input.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use crate::{
        dialect::{mysql_renderer::MySqlDialectRenderer, DialectRenderer},
        domain::canonical::{
            CanonicalColumn, CanonicalRow, CanonicalTable, CanonicalValue, LogicalType,
        },
    };

    fn sample_table() -> CanonicalTable {
        CanonicalTable {
            name: "users".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                },
                CanonicalColumn {
                    name: "name".to_string(),
                    logical_type: LogicalType::String,
                    nullable: false,
                },
                CanonicalColumn {
                    name: "active".to_string(),
                    logical_type: LogicalType::Boolean,
                    nullable: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        }
    }

    fn sample_table_with_schema() -> CanonicalTable {
        let mut table = sample_table();
        table.name = "app.users".to_string();
        table
    }

    #[test]
    fn render_table_ddl_contains_pk() {
        let renderer = MySqlDialectRenderer;
        let ddl = renderer.render_table_ddl(&sample_table()).unwrap();
        assert!(ddl.contains("CREATE TABLE `users`"));
        assert!(ddl.contains("`id` BIGINT NOT NULL"));
        assert!(ddl.contains("PRIMARY KEY (`id`)"));
    }

    #[test]
    fn render_insert_batch_formats_literals() {
        let renderer = MySqlDialectRenderer;
        let sql = renderer
            .render_insert_batch(
                &sample_table(),
                &[CanonicalRow {
                    values: vec![
                        CanonicalValue::Integer(1),
                        CanonicalValue::String("O'Reilly".to_string()),
                        CanonicalValue::Boolean(true),
                    ],
                }],
            )
            .unwrap();

        assert!(sql.contains("INSERT INTO `users` (`id`, `name`, `active`) VALUES"));
        assert!(sql.contains("(1, 'O''Reilly', 1)"));
    }

    #[test]
    fn render_table_ddl_quotes_schema_qualified_name() {
        let renderer = MySqlDialectRenderer;
        let ddl = renderer
            .render_table_ddl(&sample_table_with_schema())
            .expect("schema-qualified table should render");
        assert!(ddl.contains("CREATE TABLE `app`.`users`"));
    }
}
