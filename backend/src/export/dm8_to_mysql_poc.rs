use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::{
    db::schema::{self, decode_cell},
    dialect::{mysql_renderer::MySqlDialectRenderer, DialectRenderer},
    domain::canonical::{canonical_table_from_details, CanonicalRow, CanonicalValue, LogicalType},
    export::{
        data::{has_lob_columns, topological_sort_by_foreign_keys},
        orchestrator::LegacyExportPlan,
    },
    models::ForeignKey,
};

pub fn export_dm8_to_mysql_ddl(connection: &Connection<'_>, plan: &LegacyExportPlan) -> Result<()> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let renderer = MySqlDialectRenderer;

    writeln!(writer, "-- DM8 -> MySQL DDL export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    for (idx, table_id) in plan.tables.iter().enumerate() {
        let details = schema::get_table_details(connection, &plan.source_schema, &table_id.name)
            .with_context(|| format!("Failed to inspect DM8 table '{}'", table_id))?;
        let mut table = canonical_table_from_details(&details);
        table.name = format!("{}.{}", plan.target_schema, details.name);
        let ddl = renderer.render_table_ddl(&table)?;
        if idx > 0 {
            writeln!(writer)?;
        }
        writeln!(writer, "{}", ddl)?;
    }

    writer
        .flush()
        .context("Failed to flush dm8->mysql ddl export")?;
    Ok(())
}

pub fn export_dm8_to_mysql_data(
    connection: &Connection<'_>,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let renderer = MySqlDialectRenderer;
    let mut total_rows = 0usize;

    writeln!(writer, "-- DM8 -> MySQL data export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    // Phase 0: Collect table details and FK info for all tables
    let mut all_details = Vec::with_capacity(plan.tables.len());
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());

    for table_id in &plan.tables {
        let details = schema::get_table_details(connection, &plan.source_schema, &table_id.name)
            .with_context(|| format!("Failed to inspect DM8 table '{}'", table_id))?;
        table_names.push(details.name.clone());
        table_fks.push(details.foreign_keys.clone());
        all_details.push(details);
    }

    // Compute FK-aware ordering
    let insert_order = topological_sort_by_foreign_keys(&table_names, &table_fks);
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    // Phase 1: TRUNCATE all tables (children first)
    writeln!(writer, "-- Phase 1: TRUNCATE tables (children before parents)")?;
    for &idx in &truncate_order {
        let target_name = format!(
            "{}.{}",
            plan.target_schema, all_details[idx].name
        );
        writeln!(writer, "TRUNCATE TABLE {};", mysql_quote_qualified(&target_name))?;
    }
    writeln!(writer)?;

    // Phase 2: INSERT data (parents first)
    writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;
    for &idx in &insert_order {
        let details = &all_details[idx];
        let source_table = canonical_table_from_details(details);
        let mut target_table = source_table.clone();
        target_table.name = format!("{}.{}", plan.target_schema, details.name);

        let count = if has_lob_columns(details) {
            export_table_rows_rowwise(
                connection,
                &plan.source_schema,
                &source_table,
                &target_table,
                &renderer,
                &mut writer,
                batch_size.max(1),
            )
            .with_context(|| format!("Failed to export LOB data for table '{}'", plan.tables[idx]))?
        } else {
            export_table_rows(
                connection,
                &plan.source_schema,
                &source_table,
                &target_table,
                &renderer,
                &mut writer,
                batch_size.max(1),
            )
            .with_context(|| format!("Failed to export data for table '{}'", plan.tables[idx]))?
        };

        if count > 0 {
            writeln!(writer)?;
        }
        total_rows += count;
    }

    writer
        .flush()
        .context("Failed to flush dm8->mysql data export")?;
    Ok(total_rows)
}

fn export_table_rows(
    connection: &Connection<'_>,
    source_schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &MySqlDialectRenderer,
    writer: &mut impl Write,
    batch_size: usize,
) -> Result<usize> {
    let source_qualified_table = format!(
        "{}.{}",
        source_schema.trim().to_uppercase(),
        source_table.name.trim().to_uppercase()
    );
    let source_ident = quote_qualified_identifier(&source_qualified_table);
    let select_columns = source_table
        .columns
        .iter()
        .map(|col| quote_identifier(&col.name))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {} FROM {}", select_columns, source_ident);

    let mut cursor = match connection.execute(&sql, ())? {
        Some(cursor) => cursor,
        None => return Ok(0),
    };

    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;
    let mut batch_rows = Vec::with_capacity(batch_size);
    let mut total_rows = 0usize;

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let mut values = Vec::with_capacity(source_table.columns.len());
            for (col_index, column) in source_table.columns.iter().enumerate() {
                let raw = decode_cell(batch, col_index, row_index);
                values.push(parse_dm8_value(&column.logical_type, raw));
            }
            batch_rows.push(CanonicalRow { values });
            total_rows += 1;

            if batch_rows.len() >= batch_size {
                writeln!(
                    writer,
                    "{}",
                    renderer.render_insert_batch(target_table, &batch_rows)?
                )?;
                batch_rows.clear();
            }
        }
    }

    if !batch_rows.is_empty() {
        writeln!(
            writer,
            "{}",
            renderer.render_insert_batch(target_table, &batch_rows)?
        )?;
    }

    Ok(total_rows)
}

fn parse_dm8_value(logical_type: &LogicalType, raw: Option<String>) -> CanonicalValue {
    let Some(raw) = raw else {
        return CanonicalValue::Null;
    };

    match logical_type {
        LogicalType::Integer => raw
            .trim()
            .parse::<i64>()
            .map(CanonicalValue::Integer)
            .unwrap_or_else(|_| CanonicalValue::Decimal(raw)),
        LogicalType::Decimal => CanonicalValue::Decimal(raw),
        LogicalType::Float => raw
            .trim()
            .parse::<f64>()
            .map(CanonicalValue::Float)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Boolean => {
            let normalized = raw.trim().to_ascii_lowercase();
            let value = matches!(normalized.as_str(), "1" | "true" | "t" | "y" | "yes");
            CanonicalValue::Boolean(value)
        }
        LogicalType::Binary => {
            if let Some(decoded) = parse_hex_bytes(&raw) {
                CanonicalValue::Binary(decoded)
            } else {
                CanonicalValue::Binary(raw.into_bytes())
            }
        }
        LogicalType::Date => CanonicalValue::Date(raw),
        LogicalType::DateTime => CanonicalValue::DateTime(raw),
        LogicalType::Json => CanonicalValue::Json(raw),
        LogicalType::String | LogicalType::Text | LogicalType::Unknown => {
            CanonicalValue::String(raw)
        }
    }
}

fn parse_hex_bytes(raw: &str) -> Option<Vec<u8>> {
    let normalized = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if normalized.is_empty() {
        return Some(Vec::new());
    }
    if normalized.len() % 2 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(normalized.len() / 2);
    for idx in (0..normalized.len()).step_by(2) {
        let byte = u8::from_str_radix(&normalized[idx..idx + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_qualified_identifier(name: &str) -> String {
    name.split('.')
        .map(quote_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

/// Quote a qualified name with MySQL backtick syntax.
fn mysql_quote_qualified(name: &str) -> String {
    name.split('.')
        .map(|part| format!("`{}`", part.replace('`', "``")))
        .collect::<Vec<_>>()
        .join(".")
}

/// Row-by-row export for tables with LOB columns (BLOB/CLOB) to avoid
/// TextRowSet truncation.
fn export_table_rows_rowwise(
    connection: &Connection<'_>,
    source_schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &MySqlDialectRenderer,
    writer: &mut impl Write,
    batch_size: usize,
) -> Result<usize> {
    let source_qualified_table = format!(
        "{}.{}",
        source_schema.trim().to_uppercase(),
        source_table.name.trim().to_uppercase()
    );
    let source_ident = quote_qualified_identifier(&source_qualified_table);
    let select_columns = source_table
        .columns
        .iter()
        .map(|col| quote_identifier(&col.name))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {} FROM {}", select_columns, source_ident);

    tracing::debug!(
        "Using row-by-row export for {} (has LOB columns)",
        source_qualified_table
    );

    let mut cursor = match connection.execute(&sql, ())? {
        Some(cursor) => cursor,
        None => return Ok(0),
    };

    let mut batch_rows = Vec::with_capacity(batch_size);
    let mut total_rows = 0usize;
    let mut col_buf: Vec<u8> = Vec::with_capacity(64 * 1024);

    while let Some(mut row) = cursor.next_row()? {
        let mut values = Vec::with_capacity(source_table.columns.len());
        for (col_index, column) in source_table.columns.iter().enumerate() {
            let col_num = (col_index + 1) as u16;

            if column.logical_type == LogicalType::Binary {
                col_buf.clear();
                let has_data = row.get_binary(col_num, &mut col_buf)
                    .with_context(|| format!(
                        "Failed to get binary data for column '{}' in table '{}'",
                        column.name, source_qualified_table
                    ))?;
                if !has_data {
                    values.push(CanonicalValue::Null);
                } else {
                    values.push(CanonicalValue::Binary(col_buf.clone()));
                }
            } else {
                col_buf.clear();
                let has_data = row.get_text(col_num, &mut col_buf)
                    .with_context(|| format!(
                        "Failed to get text data for column '{}' in table '{}'",
                        column.name, source_qualified_table
                    ))?;
                if !has_data {
                    values.push(CanonicalValue::Null);
                } else {
                    let text = match std::str::from_utf8(&col_buf) {
                        Ok(s) => s.to_string(),
                        Err(_) => encoding_rs::GB18030.decode(&col_buf).0.into_owned(),
                    };
                    values.push(parse_dm8_value(&column.logical_type, Some(text)));
                }
            }
        }

        batch_rows.push(CanonicalRow { values });
        total_rows += 1;

        if batch_rows.len() >= batch_size {
            writeln!(
                writer,
                "{}",
                renderer.render_insert_batch(target_table, &batch_rows)?
            )?;
            batch_rows.clear();
        }
    }

    if !batch_rows.is_empty() {
        writeln!(
            writer,
            "{}",
            renderer.render_insert_batch(target_table, &batch_rows)?
        )?;
    }

    Ok(total_rows)
}

#[cfg(test)]
mod tests {
    use super::{parse_dm8_value, parse_hex_bytes, quote_qualified_identifier};
    use crate::domain::canonical::{CanonicalValue, LogicalType};

    #[test]
    fn parse_hex_bytes_supports_prefixed_input() {
        assert_eq!(parse_hex_bytes("0x0AFF"), Some(vec![0x0A, 0xFF]));
        assert_eq!(parse_hex_bytes(""), Some(vec![]));
        assert_eq!(parse_hex_bytes("ABC"), None);
    }

    #[test]
    fn parse_dm8_value_maps_numeric_and_bool() {
        assert_eq!(
            parse_dm8_value(&LogicalType::Integer, Some("42".to_string())),
            CanonicalValue::Integer(42)
        );
        assert_eq!(
            parse_dm8_value(&LogicalType::Boolean, Some("Y".to_string())),
            CanonicalValue::Boolean(true)
        );
        assert_eq!(
            parse_dm8_value(&LogicalType::Boolean, Some("0".to_string())),
            CanonicalValue::Boolean(false)
        );
    }

    #[test]
    fn quote_qualified_identifier_quotes_each_part() {
        assert_eq!(
            quote_qualified_identifier("SYSDBA.USERS"),
            "\"SYSDBA\".\"USERS\""
        );
    }
}
