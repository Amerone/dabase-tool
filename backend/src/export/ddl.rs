use std::{
    collections::HashSet,
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use odbc_api::Connection;

use crate::{
    db::schema::get_table_details,
    models::{Column, TableDetails},
};

pub fn generate_create_table(table: &TableDetails) -> String {
    let table_ident = quote_identifier(&table.name);

    let column_lines = table
        .columns
        .iter()
        .map(|col| format!("    {}", format_column_definition(col)))
        .collect::<Vec<_>>()
        .join(",\n");

    let mut ddl = String::new();
    let _ = writeln!(
        ddl,
        "CREATE TABLE {} (\n{}\n);",
        table_ident, column_lines
    );

    if let Some(comment) = table.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        let _ = writeln!(
            ddl,
            "COMMENT ON TABLE {} IS '{}';",
            table_ident,
            escape_single_quotes(comment)
        );
    }

    for column in &table.columns {
        if let Some(comment) = column.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            let _ = writeln!(
                ddl,
                "COMMENT ON COLUMN {}.{} IS '{}';",
                table_ident,
                quote_identifier(&column.name),
                escape_single_quotes(comment)
            );
        }
    }

    ddl.trim_end().to_string()
}

pub fn generate_primary_key(table: &TableDetails) -> Option<String> {
    if table.primary_keys.is_empty() {
        return None;
    }

    let columns = table
        .primary_keys
        .iter()
        .map(|s| quote_identifier(s))
        .collect::<Vec<_>>()
        .join(", ");

    let base_name = table
        .name
        .rsplit('.')
        .next()
        .unwrap_or(&table.name);
    let constraint_name = format!("PK_{}", base_name);

    Some(format!(
        "ALTER TABLE {} ADD CONSTRAINT {} PRIMARY KEY ({});",
        quote_identifier(&table.name),
        quote_identifier(&constraint_name),
        columns
    ))
}

pub fn generate_indexes(table: &TableDetails) -> Vec<String> {
    let pk_set: HashSet<String> = table
        .primary_keys
        .iter()
        .map(|col| col.to_uppercase())
        .collect();

    table
        .indexes
        .iter()
        .filter_map(|index| {
            if index.columns.is_empty() {
                return None;
            }

            let index_cols_upper: HashSet<String> =
                index.columns.iter().map(|c| c.to_uppercase()).collect();

            if index.unique && !pk_set.is_empty() && index_cols_upper == pk_set {
                return None;
            }

            let columns = index
                .columns
                .iter()
                .map(|s| quote_identifier(s))
                .collect::<Vec<_>>()
                .join(", ");

            let prefix = if index.unique {
                "CREATE UNIQUE INDEX"
            } else {
                "CREATE INDEX"
            };

            Some(format!(
                "{} {} ON {} ({});",
                prefix,
                quote_identifier(&index.name),
                quote_identifier(&table.name),
                columns
            ))
        })
        .collect()
}

pub fn export_schema_ddl(
    connection: &Connection<'_>,
    schema: &str,
    tables: &[String],
    output_path: &Path,
) -> Result<()> {
    let schema = schema.to_uppercase();

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                output_path.display()
            )
        })?;
    }

    let file = File::create(output_path).with_context(|| {
        format!("Failed to create DDL export file at {}", output_path.display())
    })?;
    let mut writer = BufWriter::new(file);

    for (i, table_name) in tables.iter().enumerate() {
        let table_details =
            get_table_details(connection, &schema, table_name).with_context(|| {
                format!("Failed to fetch table metadata for '{}'", table_name)
            })?;

        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", schema, table_details.name);

        if i > 0 {
            writeln!(writer)?;
        }

        writeln!(
            writer,
            "-- Table: {}",
            quote_identifier(&render_table.name)
        )?;
        writeln!(writer, "{}", generate_create_table(&render_table))?;

        if let Some(pk_stmt) = generate_primary_key(&render_table) {
            writeln!(writer)?;
            writeln!(writer, "{}", pk_stmt)?;
        }

        let index_statements = generate_indexes(&render_table);
        if !index_statements.is_empty() {
            writeln!(writer)?;
            for stmt in index_statements {
                writeln!(writer, "{}", stmt)?;
            }
        }
    }

    writer.flush().context("Failed to flush DDL export to disk")?;
    Ok(())
}

fn format_column_definition(column: &Column) -> String {
    let mut definition = format!(
        "{} {}",
        quote_identifier(&column.name),
        format_data_type(column)
    );

    let nullability = if column.nullable { "NULL" } else { "NOT NULL" };
    definition.push(' ');
    definition.push_str(nullability);

    definition
}

fn format_data_type(column: &Column) -> String {
    let mut data_type = column.data_type.trim().to_uppercase();

    if data_type.contains('(') {
        return data_type;
    }

    if let Some(length) = column.length {
        if length > 0 {
            match data_type.as_str() {
                "VARCHAR" | "VARCHAR2" | "CHAR" | "NCHAR" | "NVARCHAR" | "NVARCHAR2" | "RAW"
                | "BINARY" => {
                    data_type = format!("{}({})", data_type, length);
                }
                "NUMBER" | "DECIMAL" | "NUMERIC" => {
                    data_type = format!("{}({})", data_type, length);
                }
                _ => {}
            }
        }
    }

    data_type
}

fn quote_identifier(identifier: &str) -> String {
    identifier
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "''")
}
