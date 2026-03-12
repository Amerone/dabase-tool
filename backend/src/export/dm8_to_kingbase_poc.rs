use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::{
    db::schema::{self, decode_cell},
    dialect::{renderer_for, DialectRenderer},
    domain::canonical::{canonical_table_from_details, CanonicalRow, CanonicalValue, LogicalType},
    export::{
        data::{has_lob_columns, topological_sort_by_foreign_keys},
        orchestrator::LegacyExportPlan,
    },
    models::{DbType, ForeignKey},
};

pub fn export_dm8_to_kingbase_ddl(
    connection: &Connection<'_>,
    plan: &LegacyExportPlan,
) -> Result<()> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let renderer = renderer_for(&DbType::Kingbase);

    writeln!(writer, "-- DM8 -> KingBase DDL export script")?;
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
        .context("Failed to flush dm8->kingbase ddl export")?;
    Ok(())
}

pub fn export_dm8_to_kingbase_data(
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
    let renderer = renderer_for(&DbType::Kingbase);
    let mut total_rows = 0usize;

    writeln!(writer, "-- DM8 -> KingBase data export script")?;
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
        let target_name = format!("{}.{}", plan.target_schema, all_details[idx].name);
        writeln!(writer, "TRUNCATE TABLE {};", quote_qualified_identifier(&target_name))?;
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
                renderer.as_ref(),
                &mut writer,
                batch_size,
            )?
        } else {
            export_table_rows(
                connection,
                &plan.source_schema,
                &source_table,
                &target_table,
                renderer.as_ref(),
                &mut writer,
                batch_size,
            )?
        };
        total_rows += count;
    }

    writer
        .flush()
        .context("Failed to flush dm8->kingbase data export")?;
    Ok(total_rows)
}

fn export_table_rows(
    connection: &Connection<'_>,
    schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &dyn DialectRenderer,
    writer: &mut BufWriter<File>,
    batch_size: usize,
) -> Result<usize> {
    let select_cols = source_table
        .columns
        .iter()
        .map(|col| {
            let ident = quote_identifier(&col.name);
            if col.logical_type == LogicalType::Binary {
                format!("RAWTOHEX({}) AS {}", ident, ident)
            } else {
                ident
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let source_ident = format!(
        "{}.{}",
        quote_identifier(schema),
        quote_identifier(&source_table.name)
    );
    let sql = format!("SELECT {} FROM {}", select_cols, source_ident);

    let mut cursor = connection
        .execute(&sql, ())
        .with_context(|| format!("Failed to query DM8 table data: {}", source_table.name))?
        .ok_or_else(|| anyhow::anyhow!("No cursor for data query"))?;

    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(16384))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut total_rows = 0usize;
    while let Some(batch) = row_set_cursor.fetch()? {
        let num_rows = batch.num_rows();
        if num_rows == 0 {
            break;
        }

        let mut rows = Vec::new();
        for row_index in 0..num_rows {
            let mut values = Vec::new();
            for (col_index, col) in source_table.columns.iter().enumerate() {
                let cell_value = decode_cell(batch, col_index, row_index);
                let canonical_value = parse_dm8_value(&col.logical_type, cell_value);
                values.push(canonical_value);
            }
            rows.push(CanonicalRow { values });
        }

        let insert_sql = renderer.render_insert_batch(target_table, &rows)?;
        writeln!(writer, "{}", insert_sql)?;
        total_rows += num_rows;
    }

    Ok(total_rows)
}

fn parse_dm8_value(logical_type: &LogicalType, raw: Option<String>) -> CanonicalValue {
    match raw {
        None => CanonicalValue::Null,
        Some(s) if s.trim().is_empty() => match logical_type {
            LogicalType::String | LogicalType::Text => CanonicalValue::String(String::new()),
            _ => CanonicalValue::Null,
        },
        Some(s) => match logical_type {
            LogicalType::Integer => s
                .parse::<i64>()
                .map(CanonicalValue::Integer)
                .unwrap_or(CanonicalValue::Null),
            LogicalType::Decimal => CanonicalValue::Decimal(s),
            LogicalType::Float => s
                .parse::<f64>()
                .map(CanonicalValue::Float)
                .unwrap_or(CanonicalValue::Null),
            LogicalType::Boolean => {
                let normalized = s.trim().to_uppercase();
                let is_true = normalized == "Y"
                    || normalized == "1"
                    || normalized == "TRUE"
                    || normalized == "T";
                CanonicalValue::Boolean(is_true)
            }
            LogicalType::Binary => parse_hex_bytes(&s)
                .map(CanonicalValue::Binary)
                .unwrap_or(CanonicalValue::Null),
            LogicalType::Date => CanonicalValue::Date(s),
            LogicalType::DateTime => CanonicalValue::DateTime(s),
            LogicalType::Json => CanonicalValue::Json(s),
            LogicalType::String | LogicalType::Text => CanonicalValue::String(s),
            LogicalType::Unknown => CanonicalValue::String(s),
        },
    }
}

fn parse_hex_bytes(hex_str: &str) -> Option<Vec<u8>> {
    let trimmed = hex_str.trim().trim_start_matches("0x");
    if trimmed.is_empty() {
        return Some(vec![]);
    }
    if trimmed.len() % 2 != 0 {
        return None;
    }
    (0..trimmed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).ok())
        .collect()
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

/// Row-by-row export for tables with LOB columns to avoid TextRowSet truncation.
fn export_table_rows_rowwise(
    connection: &Connection<'_>,
    schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &dyn DialectRenderer,
    writer: &mut BufWriter<File>,
    batch_size: usize,
) -> Result<usize> {
    let source_qualified_table = format!(
        "{}.{}",
        schema.trim().to_uppercase(),
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

    let mut cursor = connection
        .execute(&sql, ())
        .with_context(|| format!("Failed to query DM8 table data: {}", source_table.name))?
        .ok_or_else(|| anyhow::anyhow!("No cursor for data query"))?;

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
            let insert_sql = renderer.render_insert_batch(target_table, &batch_rows)?;
            writeln!(writer, "{}", insert_sql)?;
            batch_rows.clear();
        }
    }

    if !batch_rows.is_empty() {
        let insert_sql = renderer.render_insert_batch(target_table, &batch_rows)?;
        writeln!(writer, "{}", insert_sql)?;
    }

    Ok(total_rows)
}
