use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::db::schema::{fetch_row_count, fetch_sequences, get_table_details};

pub fn export_table_data(
    connection: &Connection<'_>,
    source_schema: &str,
    target_schema: &str,
    table: &str,
    writer: &mut impl Write,
    batch_size: usize,
) -> Result<usize> {
    let source_schema_upper = source_schema.to_uppercase();
    let target_schema_upper = target_schema.to_uppercase();
    let table_upper = table.to_uppercase();
    let source_qualified_table = format!("{}.{}", source_schema_upper, table_upper);
    let target_qualified_table = format!("{}.{}", target_schema_upper, table_upper);
    let source_ident = quote_identifier(&source_qualified_table);
    let target_ident = quote_identifier(&target_qualified_table);

    let table_details = get_table_details(connection, &source_schema_upper, &table_upper)
        .with_context(|| format!("Failed to get table details for {}", source_qualified_table))?;

    let query = format!("SELECT * FROM {}", source_ident);

    let mut cursor = match connection.execute(&query, ())? {
        Some(cursor) => cursor,
        None => {
            tracing::info!("No data to export for table {}", source_qualified_table);
            return Ok(0);
        }
    };

    let mut batch = Vec::new();
    let mut row_count = 0;
    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    while let Some(batch_result) = row_set_cursor.fetch()? {
        for row_index in 0..batch_result.num_rows() {
            let mut values = Vec::new();

            for (col_index, column) in table_details.columns.iter().enumerate() {
                let value = batch_result.at_as_str(col_index, row_index)?;

                let formatted_value = match value {
                    None => "NULL".to_string(),
                    Some(v) => format_literal(&column.data_type, v),
                };

                values.push(formatted_value);
            }

            batch.push(format!("({})", values.join(", ")));
            row_count += 1;

            if batch.len() >= batch_size {
                write_batch(writer, &target_ident, &batch)?;
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        write_batch(writer, &target_ident, &batch)?;
    }

    tracing::info!(
        "Exported {} rows from {}",
        row_count,
        source_qualified_table
    );
    Ok(row_count)
}

pub fn export_schema_data(
    connection: &Connection<'_>,
    source_schema: &str,
    target_schema: &str,
    tables: &[String],
    output_path: &Path,
    batch_size: usize,
    include_row_counts: bool,
) -> Result<usize> {
    let source_schema_upper = source_schema.to_uppercase();
    let target_schema_upper = target_schema.to_uppercase();
    let sequences = fetch_sequences(connection, &source_schema_upper).unwrap_or_default();

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                output_path.display()
            )
        })?;
    }

    let file = File::create(output_path).with_context(|| {
        format!("Failed to create data export file at {}", output_path.display())
    })?;
    let mut writer = BufWriter::new(file);

    // Pre-compute row counts for header (optional)
    let mut total_rows: i64 = 0;
    let mut table_row_counts = Vec::new();
    if include_row_counts {
        for table in tables {
            match fetch_row_count(connection, &source_schema_upper, table) {
                Ok(cnt) => {
                    total_rows += cnt;
                    table_row_counts.push((table.clone(), Some(cnt)));
                }
                Err(_) => table_row_counts.push((table.clone(), None)),
            }
        }
    } else {
        for table in tables {
            table_row_counts.push((table.clone(), None));
        }
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    writeln!(writer, "-- DM8 Data Export")?;
    writeln!(writer, "-- Tables: {}", tables.len())?;
    if include_row_counts {
        writeln!(writer, "-- Rows (estimated): {}", total_rows)?;
    } else {
        writeln!(writer, "-- Rows (estimated): skipped (per request)")?;
    }
    writeln!(writer, "-- Generated at: {}", timestamp)?;
    writeln!(writer, "-- Warning: This script truncates tables before inserting data.")?;
    if !sequences.is_empty() {
        writeln!(writer, "-- Sequences will be reset to START values before inserts")?;
    }
    writeln!(writer)?;

    if !sequences.is_empty() {
        writeln!(writer, "-- Reset sequences")?;
        for seq in &sequences {
            let start = seq.start_with.unwrap_or(1);
            writeln!(
                writer,
                "ALTER SEQUENCE {} RESTART WITH {};",
                quote_identifier(&format!("{}.{}", target_schema_upper, seq.name)),
                start
            )?;
        }
        writeln!(writer)?;
    }

    let mut exported_total: usize = 0;

    for (i, (table_name, expected_rows)) in table_row_counts.iter().enumerate() {
        if i > 0 {
            writeln!(writer)?;
        }

        writeln!(
            writer,
            "-- Data for table: {}.{}{}",
            target_schema_upper,
            table_name.to_uppercase(),
            expected_rows
                .map(|c| format!(" ({} rows)", c))
                .unwrap_or_else(|| " (rows unknown)".to_string())
        )?;
        let qualified = quote_identifier(&format!(
            "{}.{}",
            target_schema_upper,
            table_name.to_uppercase()
        ));
        writeln!(writer, "TRUNCATE TABLE {};", qualified)?;

        // Reset identity columns if present (read from source, apply to target)
        if let Ok(details) = get_table_details(connection, &source_schema_upper, table_name) {
            let identity_cols: Vec<_> = details.columns.iter().filter(|c| c.identity).collect();
            for col in identity_cols {
                let start = col.identity_start.unwrap_or(1);
                writeln!(
                    writer,
                    "ALTER TABLE {} ALTER COLUMN {} RESTART WITH {};",
                    qualified,
                    quote_identifier(&col.name),
                    start
                )?;
            }
        }

        let count = export_table_data(
            connection,
            &source_schema_upper,
            &target_schema_upper,
            table_name,
            &mut writer,
            batch_size,
        )
        .with_context(|| format!("Failed to export data for table '{}'", table_name))?;

        exported_total += count;
    }

    writer.flush().context("Failed to flush data export to disk")?;
    Ok(exported_total)
}

fn write_batch(writer: &mut impl Write, table: &str, batch: &[String]) -> Result<()> {
    writeln!(
        writer,
        "INSERT INTO {} VALUES\n{};",
        table,
        batch.join(",\n")
    )?;
    Ok(())
}

fn is_numeric_type(data_type: &str) -> bool {
    let upper = data_type.to_uppercase();
    matches!(
        upper.as_str(),
        "NUMBER" | "INTEGER" | "INT" | "SMALLINT" | "BIGINT" | "DECIMAL" | "NUMERIC" | "FLOAT" | "DOUBLE" | "REAL"
    )
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "''")
}

fn quote_identifier(identifier: &str) -> String {
    identifier
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn is_date_type(dt: &str) -> bool {
    matches!(dt.to_uppercase().as_str(), "DATE")
}

fn is_timestamp_type(dt: &str) -> bool {
    matches!(
        dt.to_uppercase().as_str(),
        "TIMESTAMP" | "TIMESTAMP WITH TIME ZONE" | "TIMESTAMP WITH LOCAL TIME ZONE"
    )
}

fn is_binary_type(dt: &str) -> bool {
    matches!(dt.to_uppercase().as_str(), "RAW" | "BINARY" | "VARBINARY" | "BLOB")
}

fn format_literal(data_type: &str, raw: &str) -> String {
    let upper = data_type.to_uppercase();
    if is_numeric_type(&upper) {
        return raw.to_string();
    }
    if is_binary_type(&upper) {
        let trimmed = raw.trim_start_matches("0x").trim_start_matches("0X");
        return format!("HEXTORAW('{}')", trimmed);
    }
    if is_date_type(&upper) {
        return format!(
            "TO_DATE('{}','YYYY-MM-DD HH24:MI:SS')",
            escape_single_quotes(raw)
        );
    }
    if is_timestamp_type(&upper) {
        return format!(
            "TO_TIMESTAMP('{}','YYYY-MM-DD HH24:MI:SS.FF')",
            escape_single_quotes(raw)
        );
    }
    format!("'{}'", escape_single_quotes(raw))
}
