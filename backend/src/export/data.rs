use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::db::schema::{fetch_row_count, fetch_sequences, get_table_details};
use crate::models::TableDetails;

pub fn export_table_data(
    connection: &Connection<'_>,
    source_schema: &str,
    target_schema: &str,
    table: &str,
    table_details: &TableDetails,
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

    let column_idents: Vec<String> = table_details
        .columns
        .iter()
        .map(|col| quote_identifier(&col.name))
        .collect();

    // Use explicit column list to ensure SELECT and INSERT column order match
    let select_columns = column_idents.join(", ");
    let query = format!("SELECT {} FROM {}", select_columns, source_ident);

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
                write_batch(writer, &target_ident, &column_idents, &batch)?;
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        write_batch(writer, &target_ident, &column_idents, &batch)?;
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
        writeln!(writer, "-- Reset sequences (DM8 uses CURRENT VALUE, not RESTART WITH)")?;
        for seq in &sequences {
            let start = seq.start_with.unwrap_or(1);
            writeln!(
                writer,
                "ALTER SEQUENCE {} CURRENT VALUE {};",
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

        let table_upper = table_name.to_uppercase();
        let source_qualified = format!("{}.{}", source_schema_upper, table_upper);
        let table_details = get_table_details(connection, &source_schema_upper, &table_upper)
            .with_context(|| format!("Failed to get table details for {}", source_qualified))?;
        let has_identity = table_details.columns.iter().any(|col| col.identity);

        writeln!(
            writer,
            "-- Data for table: {}.{}{}",
            target_schema_upper,
            table_upper,
            expected_rows
                .map(|c| format!(" ({} rows)", c))
                .unwrap_or_else(|| " (rows unknown)".to_string())
        )?;
        let qualified = quote_identifier(&format!("{}.{}", target_schema_upper, table_upper));
        // TRUNCATE TABLE resets IDENTITY columns to their original seed value in DM8
        writeln!(writer, "TRUNCATE TABLE {};", qualified)?;

        if has_identity {
            write_identity_insert(&mut writer, &qualified, true)?;
        }

        let count = export_table_data(
            connection,
            &source_schema_upper,
            &target_schema_upper,
            table_name,
            &table_details,
            &mut writer,
            batch_size,
        )
        .with_context(|| format!("Failed to export data for table '{}'", table_name))?;

        if has_identity {
            write_identity_insert(&mut writer, &qualified, false)?;
        }

        exported_total += count;
    }

    writer.flush().context("Failed to flush data export to disk")?;
    Ok(exported_total)
}

fn write_batch(
    writer: &mut impl Write,
    table: &str,
    columns: &[String],
    batch: &[String],
) -> Result<()> {
    writeln!(
        writer,
        "INSERT INTO {} ({}) VALUES\n{};",
        table,
        columns.join(", "),
        batch.join(",\n")
    )?;
    Ok(())
}

fn write_identity_insert(writer: &mut impl Write, table: &str, enabled: bool) -> Result<()> {
    let mode = if enabled { "ON" } else { "OFF" };
    writeln!(writer, "SET IDENTITY_INSERT {} {};", table, mode)?;
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

/// Normalize ISO 8601 timestamp to DM8-compatible format.
/// Handles: T→space, comma→dot, Z→+00:00, +HH→+HH:00, +HHMM→+HH:MM
fn normalize_iso8601_timestamp(raw: &str) -> String {
    let mut normalized = raw.replace('T', " ");
    // ISO 8601 allows comma as decimal separator
    if normalized.contains(',') {
        normalized = normalized.replace(',', ".");
    }
    // Handle Z suffix (UTC)
    if normalized.ends_with('Z') || normalized.ends_with('z') {
        normalized.pop();
        normalized.push_str("+00:00");
        return normalized;
    }
    // Normalize timezone offset formats: +HH → +HH:00, +HHMM → +HH:MM
    if let Some(pos) = normalized.rfind(|c| c == '+' || c == '-') {
        // Only process if this is after the time part (contains :)
        if normalized[..pos].contains(':') {
            let sign = &normalized[pos..pos + 1];
            let offset = &normalized[pos + 1..];
            if offset.len() == 2 && offset.chars().all(|c| c.is_ascii_digit()) {
                // +HH or -HH → +HH:00 or -HH:00
                normalized = format!("{}{}{}:00", &normalized[..pos], sign, offset);
            } else if offset.len() == 4 && offset.chars().all(|c| c.is_ascii_digit()) {
                // +HHMM or -HHMM → +HH:MM or -HH:MM
                normalized = format!(
                    "{}{}{}:{}",
                    &normalized[..pos],
                    sign,
                    &offset[..2],
                    &offset[2..]
                );
            }
        }
    }
    normalized
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
        // Choose format based on actual value content
        let format_str = if raw.contains(':') {
            "YYYY-MM-DD HH24:MI:SS"
        } else {
            "YYYY-MM-DD"
        };
        return format!(
            "TO_DATE('{}','{}')",
            escape_single_quotes(raw),
            format_str
        );
    }
    if is_timestamp_type(&upper) {
        // Normalize ISO 8601 format to DM8-compatible format
        let normalized = normalize_iso8601_timestamp(raw.trim());

        // Detect timezone offset (+HH:MM or -HH:MM after time part)
        let has_tz = has_timezone_offset(&normalized);

        // Extract main part (without timezone) for format string analysis
        let main_part = if has_tz {
            normalized
                .rfind(|c| c == '+' || c == '-')
                .filter(|&pos| normalized[..pos].contains(':'))
                .map(|pos| &normalized[..pos])
                .unwrap_or(&normalized)
        } else {
            &normalized
        };

        // Build format string based on actual value content
        let mut format_str = if let Some(space_pos) = main_part.find(' ') {
            let time_part = &main_part[space_pos + 1..];
            let colon_count = time_part.chars().filter(|c| *c == ':').count();
            if colon_count >= 2 {
                "YYYY-MM-DD HH24:MI:SS".to_string()
            } else if colon_count == 1 {
                "YYYY-MM-DD HH24:MI".to_string()
            } else {
                "YYYY-MM-DD".to_string()
            }
        } else {
            "YYYY-MM-DD".to_string()
        };

        // Check for fractional seconds (. followed by digits in main part)
        if let Some(dot_pos) = main_part.rfind('.') {
            let after_dot = &main_part[dot_pos + 1..];
            if after_dot.chars().take_while(|c| c.is_ascii_digit()).count() > 0 {
                format_str.push_str(".FF");
            }
        }
        if has_tz {
            format_str.push_str(" TZH:TZM");
        }

        // Use TO_TIMESTAMP_TZ for TIMESTAMP WITH TIME ZONE types or values with timezone
        if upper.contains("TIME ZONE") || has_tz {
            return format!(
                "TO_TIMESTAMP_TZ('{}','{}')",
                escape_single_quotes(&normalized),
                format_str
            );
        }
        return format!(
            "TO_TIMESTAMP('{}','{}')",
            escape_single_quotes(&normalized),
            format_str
        );
    }
    format!("'{}'", escape_single_quotes(raw))
}

/// Check if the string has a timezone offset (+HH:MM or -HH:MM).
/// Expects normalized format from normalize_iso8601_timestamp.
fn has_timezone_offset(s: &str) -> bool {
    // Look for +HH:MM or -HH:MM pattern after the time part
    if let Some(pos) = s.rfind(|c| c == '+' || c == '-') {
        // Must be after the time part (contains :) to avoid date separators
        if !s[..pos].contains(':') {
            return false;
        }
        let offset = &s[pos + 1..];
        // Expect exactly HH:MM format (5 chars)
        if offset.len() != 5 {
            return false;
        }
        let (hh, rest) = offset.split_at(2);
        if let Some(mm) = rest.strip_prefix(':') {
            return hh.chars().all(|c| c.is_ascii_digit())
                && mm.len() == 2
                && mm.chars().all(|c| c.is_ascii_digit());
        }
    }
    false
}
