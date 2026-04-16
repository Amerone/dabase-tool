//! Unified parallel data export engine.
//!
//! Replaces the duplicated export logic in `dm8_to_shentong_poc`, `dm8_to_kingbase_poc`,
//! and `dm8_to_mysql_poc` with a single pipeline that:
//! 1. Batch-fetches metadata via the main ODBC connection
//! 2. FK-sorts tables into parallelizable layers
//! 3. Exports each layer in parallel (one thread per table, each with its own ODBC connection)
//! 4. Merges per-table temp files into the final output in FK order

use std::{
    any::Any,
    fs::{self, File},
    io::{BufWriter, Read as _, Write},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::{
    buffers::TextRowSet,
    parameter::{VarBinaryArray, VarCharArray, VarWCharArray},
    ConnectionOptions, Cursor, Environment,
};

use crate::{
    db::schema::decode_cell,
    dialect::DialectRenderer,
    domain::canonical::{canonical_table_from_details, CanonicalRow, CanonicalValue, LogicalType},
    export::{
        common::apply_identifier_case,
        data::{has_lob_columns, topological_sort_into_layers},
    },
    models::{ConnectionConfig, DbType, ForeignKey, TableDetails},
};

/// Configuration for the unified parallel data export pipeline.
pub struct DataExportPipeline {
    pub source_schema: String,
    pub target_schema: String,
    pub batch_size: usize,
    pub identifier_case: String,
    pub max_parallelism: usize,
    /// When true, skip DISABLE/ENABLE CONSTRAINT statements.
    /// Useful for cross-database migration where source FK constraint names
    /// may not exist in the target database. FK-aware ordering is still used.
    pub skip_fk_toggle: bool,
    /// When true, append CASCADE to TRUNCATE statements.
    /// Required for PostgreSQL-compatible targets (ShenTong, Kingbase)
    /// where FK constraints block plain TRUNCATE.
    pub truncate_cascade: bool,
    /// When true, skip DISABLE/ENABLE ALL TRIGGERS statements.
    /// MySQL does not support ALTER TABLE ... DISABLE ALL TRIGGERS.
    pub skip_trigger_toggle: bool,
    /// When true, emit `SET FOREIGN_KEY_CHECKS = 0/1` instead of per-table
    /// DISABLE/ENABLE CONSTRAINT. Required for MySQL where TRUNCATE on
    /// FK-referenced tables is blocked unless foreign_key_checks is off.
    pub use_foreign_key_checks: bool,
    /// When true, fetch character data through ODBC wide-character buffers.
    /// DM8's narrow ODBC buffers may replace characters that cannot be
    /// represented in the driver/client code page with `?` before Rust sees
    /// them. This is slower, so enable it only for paths that need it.
    pub unicode_safe_text: bool,
}

/// Result of exporting a single table (returned from worker threads).
struct TableExportResult {
    /// Number of rows exported
    row_count: usize,
    /// Path to the temp file containing the INSERT statements
    temp_path: PathBuf,
}

const STREAM_FETCH_CHUNK_BYTES: usize = 16 * 1024;
const STREAM_FETCH_CHUNK_U16: usize = 8 * 1024;

// PLACEHOLDER_PIPELINE_IMPL

impl DataExportPipeline {
    /// Execute the full data export pipeline.
    ///
    /// Uses the provided `connection` for metadata queries, then spawns parallel
    /// threads (each with its own ODBC connection) for the actual data export.
    /// The `renderer` controls target dialect SQL generation.
    ///
    /// Returns the total number of rows exported.
    pub fn execute(
        &self,
        connection: &odbc_api::Connection<'_>,
        connection_config: &ConnectionConfig,
        renderer: &dyn DialectRenderer,
        all_details: &[TableDetails],
        output_path: &Path,
    ) -> Result<usize> {
        let table_names: Vec<String> = all_details.iter().map(|d| d.name.clone()).collect();
        let table_fks: Vec<Vec<ForeignKey>> =
            all_details.iter().map(|d| d.foreign_keys.clone()).collect();

        // FK-aware layer sort
        let layers = topological_sort_into_layers(&table_names, &table_fks);
        // Flat insert order for TRUNCATE (reverse) and file merge
        let insert_order: Vec<usize> = layers.iter().flatten().copied().collect();
        let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

        // Build case-transformed details for target output
        let target_details: Vec<TableDetails> = all_details
            .iter()
            .map(|d| {
                let mut td = d.clone();
                apply_identifier_case(&mut td, &self.identifier_case);
                td
            })
            .collect();

        // Ensure output directory exists
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create output dir {}", parent.display()))?;
        }

        let file = File::create(output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        let mut writer = BufWriter::with_capacity(4 * 1024 * 1024, file);

        // File header
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        writeln!(writer, "-- Data export script (unified pipeline)")?;
        writeln!(writer, "-- source schema: {}", self.source_schema)?;
        writeln!(writer, "-- target schema: {}", self.target_schema)?;
        writeln!(writer, "-- generated: {}", timestamp)?;
        writeln!(writer, "-- tables: {}", all_details.len())?;
        writeln!(writer)?;

        // Phase 0: Disable FK constraints
        let has_fks = !self.skip_fk_toggle && table_fks.iter().any(|fks| !fks.is_empty());
        if self.use_foreign_key_checks {
            // MySQL: use session variable to bypass FK checks for TRUNCATE and INSERT
            writeln!(writer, "-- Phase 0: Disable FK checks (MySQL)")?;
            writeln!(writer, "SET FOREIGN_KEY_CHECKS = 0;")?;
            writeln!(writer)?;
        } else if has_fks {
            writeln!(writer, "-- Phase 0: Disable FK constraints")?;
            for (idx, fks) in table_fks.iter().enumerate() {
                for fk in fks {
                    writeln!(
                        writer,
                        "ALTER TABLE {} DISABLE CONSTRAINT {};",
                        renderer.quote_identifier(&target_details[idx].name),
                        renderer.quote_identifier(&fk.name)
                    )?;
                }
            }
            writeln!(writer)?;
        }

        // Phase 1: TRUNCATE (children first)
        let cascade_suffix = if self.truncate_cascade {
            " CASCADE"
        } else {
            ""
        };
        writeln!(
            writer,
            "-- Phase 1: TRUNCATE tables (children before parents)"
        )?;
        for &idx in &truncate_order {
            writeln!(
                writer,
                "TRUNCATE TABLE {}{};",
                renderer.quote_identifier(&target_details[idx].name),
                cascade_suffix
            )?;
        }
        writeln!(writer)?;

        // Phase 1.5: Disable ALL triggers on every table.
        // MySQL does not support this syntax, so skip when skip_trigger_toggle is set.
        if !self.skip_trigger_toggle {
            writeln!(writer, "-- Disable triggers before data import")?;
            for details in &target_details {
                writeln!(
                    writer,
                    "ALTER TABLE {} DISABLE ALL TRIGGERS;",
                    renderer.quote_identifier(&details.name),
                )?;
            }
            writeln!(writer)?;
        }

        // Phase 2: Export data layer by layer with parallelism
        writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;

        // Build ODBC connection string once for worker threads
        let conn_string = connection_config
            .odbc_connection_string()
            .context("Failed to build ODBC connection string for pipeline workers")?;
        let environment =
            Arc::new(Environment::new().context("Failed to create ODBC environment for pipeline")?);

        let temp_dir = output_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(".pipeline_tmp");
        fs::create_dir_all(&temp_dir)?;

        let mut total_rows = 0usize;

        for (layer_idx, layer) in layers.iter().enumerate() {
            tracing::debug!("Pipeline layer {}: {} tables", layer_idx, layer.len());

            // Limit parallelism within each layer
            let effective_parallelism = layer.len().min(self.max_parallelism);

            if effective_parallelism <= 1 || layer.len() == 1 {
                // Single-threaded path: export directly to main writer
                for &idx in layer {
                    let source_table = canonical_table_from_details(&all_details[idx]);
                    let target_table = canonical_table_from_details(&target_details[idx]);
                    let count = export_table_to_writer(
                        connection,
                        &self.source_schema,
                        &source_table,
                        &target_table,
                        &all_details[idx],
                        renderer,
                        &mut writer,
                        self.batch_size,
                        self.unicode_safe_text,
                    )?;
                    total_rows += count;
                }
            } else {
                // Parallel path: each thread writes to a temp file, then merge
                let results = std::thread::scope(|scope| -> Result<Vec<TableExportResult>> {
                    let mut handles = Vec::with_capacity(layer.len());

                    for &idx in layer {
                        let env = Arc::clone(&environment);
                        let cs = &conn_string;
                        let source_schema = &self.source_schema;
                        let details = &all_details[idx];
                        let target_det = &target_details[idx];
                        let batch_size = self.batch_size;
                        let tmp_dir = &temp_dir;
                        let unicode_safe_text = self.unicode_safe_text;

                        handles.push(scope.spawn(move || -> Result<TableExportResult> {
                            catch_unwind(AssertUnwindSafe(|| -> Result<TableExportResult> {
                                let temp_path = tmp_dir.join(format!("table_{}.tmp.sql", idx));
                                let temp_file = File::create(&temp_path).with_context(|| {
                                    format!("Failed to create temp file for table {}", idx)
                                })?;
                                let mut tw = BufWriter::with_capacity(1024 * 1024, temp_file);

                                // Create independent ODBC connection for this thread
                                let thread_conn = env
                                    .connect_with_connection_string(
                                        cs,
                                        ConnectionOptions::default(),
                                    )
                                    .with_context(|| {
                                        format!(
                                            "Pipeline worker failed to connect for table {}",
                                            idx
                                        )
                                    })?;

                                let source_table = canonical_table_from_details(details);
                                let target_table = canonical_table_from_details(target_det);

                                let count = export_table_to_writer(
                                    &thread_conn,
                                    source_schema,
                                    &source_table,
                                    &target_table,
                                    details,
                                    renderer,
                                    &mut tw,
                                    batch_size,
                                    unicode_safe_text,
                                )?;

                                tw.flush()?;
                                Ok(TableExportResult {
                                    row_count: count,
                                    temp_path,
                                })
                            }))
                            .map_err(|panic| {
                                anyhow::anyhow!(
                                    "Pipeline worker panicked while exporting {}.{}: {}",
                                    source_schema,
                                    details.name,
                                    panic_payload_to_string(panic)
                                )
                            })?
                        }));
                    }

                    handles
                        .into_iter()
                        .map(|h| {
                            h.join().map_err(|panic| {
                                anyhow::anyhow!(
                                    "Pipeline worker thread panicked: {}",
                                    panic_payload_to_string(panic)
                                )
                            })?
                        })
                        .collect()
                })?;

                // Merge temp files into main writer in layer order
                for result in &results {
                    let mut tmp_file = File::open(&result.temp_path).with_context(|| {
                        format!("Failed to open temp file {:?}", result.temp_path)
                    })?;
                    let mut buf = Vec::new();
                    tmp_file.read_to_end(&mut buf)?;
                    writer.write_all(&buf)?;
                    let _ = fs::remove_file(&result.temp_path);
                    total_rows += result.row_count;
                }
            }
        }

        // Cleanup temp dir
        let _ = fs::remove_dir_all(&temp_dir);

        // Re-enable triggers after data import
        if !self.skip_trigger_toggle {
            writeln!(writer)?;
            writeln!(writer, "-- Re-enable triggers after data import")?;
            for details in &target_details {
                writeln!(
                    writer,
                    "ALTER TABLE {} ENABLE ALL TRIGGERS;",
                    renderer.quote_identifier(&details.name),
                )?;
            }
        }

        // Phase 3: Re-enable FK constraints
        if self.use_foreign_key_checks {
            writeln!(writer)?;
            writeln!(writer, "-- Phase 3: Re-enable FK checks (MySQL)")?;
            writeln!(writer, "SET FOREIGN_KEY_CHECKS = 1;")?;
        } else if has_fks {
            writeln!(writer)?;
            writeln!(writer, "-- Phase 3: Re-enable FK constraints")?;
            for (idx, fks) in table_fks.iter().enumerate() {
                for fk in fks {
                    writeln!(
                        writer,
                        "ALTER TABLE {} ENABLE CONSTRAINT {};",
                        renderer.quote_identifier(&target_details[idx].name),
                        renderer.quote_identifier(&fk.name)
                    )?;
                }
            }
        }

        writer.flush().context("Failed to flush pipeline output")?;
        Ok(total_rows)
    }
}

/// Export a single table's data to a writer using the dialect renderer.
/// Dispatches to batch or row-by-row mode based on LOB column presence.
pub fn export_table_to_writer(
    connection: &odbc_api::Connection<'_>,
    source_schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    details: &TableDetails,
    renderer: &dyn DialectRenderer,
    writer: &mut impl Write,
    batch_size: usize,
    unicode_safe_text: bool,
) -> Result<usize> {
    if should_use_rowwise_export(details, source_table, unicode_safe_text) {
        export_table_rows_rowwise(
            connection,
            source_schema,
            source_table,
            target_table,
            renderer,
            writer,
            batch_size,
            unicode_safe_text,
        )
    } else {
        export_table_rows(
            connection,
            source_schema,
            source_table,
            target_table,
            renderer,
            writer,
            batch_size,
        )
    }
}

fn should_use_rowwise_export(
    details: &TableDetails,
    source_table: &crate::domain::canonical::CanonicalTable,
    unicode_safe_text: bool,
) -> bool {
    has_lob_columns(details) || (unicode_safe_text && has_character_columns(source_table))
}

fn has_character_columns(table: &crate::domain::canonical::CanonicalTable) -> bool {
    table
        .columns
        .iter()
        .any(|col| is_character_logical_type(&col.logical_type))
}

fn is_character_logical_type(logical_type: &LogicalType) -> bool {
    matches!(
        logical_type,
        LogicalType::String | LogicalType::Text | LogicalType::Json | LogicalType::Unknown
    )
}

/// Batch export using TextRowSet (fast path for non-LOB tables).
fn export_table_rows(
    connection: &odbc_api::Connection<'_>,
    schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &dyn DialectRenderer,
    writer: &mut impl Write,
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
        .with_context(|| format!("Failed to query table data: {}", source_table.name))?
        .ok_or_else(|| anyhow::anyhow!("No cursor for data query on {}", source_table.name))?;

    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(16384))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut total_rows = 0usize;
    let mut rows = Vec::with_capacity(batch_size);

    while let Some(batch) = row_set_cursor.fetch()? {
        let num_rows = batch.num_rows();
        if num_rows == 0 {
            break;
        }

        for row_index in 0..num_rows {
            let mut values = Vec::with_capacity(source_table.columns.len());
            for (col_index, col) in source_table.columns.iter().enumerate() {
                let cell_value = decode_cell(batch, col_index, row_index);
                values.push(parse_dm8_value(&col.logical_type, cell_value));
            }
            rows.push(CanonicalRow { values });
        }

        renderer.write_insert_batch(writer, target_table, &rows)?;
        total_rows += rows.len();
        rows.clear();
    }

    Ok(total_rows)
}

/// Row-by-row export for tables with LOB columns using chunked `SQLGetData`.
fn export_table_rows_rowwise(
    connection: &odbc_api::Connection<'_>,
    schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &dyn DialectRenderer,
    writer: &mut impl Write,
    batch_size: usize,
    unicode_safe_text: bool,
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
        "Pipeline: row-by-row export for {} (has LOB columns)",
        source_qualified_table
    );

    let mut cursor = connection
        .execute(&sql, ())
        .with_context(|| format!("Failed to query table data: {}", source_table.name))?
        .ok_or_else(|| anyhow::anyhow!("No cursor for data query on {}", source_table.name))?;

    let use_single_row_batches = renderer.dialect_kind() == DbType::Kingbase
        && target_table.columns.iter().any(|col| {
            matches!(
                col.logical_type,
                LogicalType::Binary | LogicalType::Text | LogicalType::Json
            )
        });
    let effective_batch_size = if use_single_row_batches {
        1
    } else {
        batch_size.max(1)
    };

    let mut batch_rows = Vec::with_capacity(effective_batch_size);
    let mut total_rows = 0usize;
    let max_inline = renderer.max_inline_blob_bytes();

    while let Some(mut row) = cursor.next_row()? {
        let mut values = Vec::with_capacity(source_table.columns.len());
        let mut large_blobs: Vec<(usize, Vec<u8>)> = Vec::new();

        for (col_index, column) in source_table.columns.iter().enumerate() {
            let col_num = (col_index + 1) as u16;

            if column.logical_type == LogicalType::Binary {
                let binary = fetch_binary_column(&mut row, col_num).with_context(|| {
                    format!(
                        "Failed to stream binary data for column '{}' in table '{}'",
                        column.name, source_qualified_table
                    )
                })?;
                if let Some(binary) = binary {
                    if binary.len() > max_inline {
                        large_blobs.push((col_index, binary));
                        values.push(CanonicalValue::Null);
                    } else {
                        values.push(CanonicalValue::Binary(binary));
                    }
                } else {
                    values.push(CanonicalValue::Null);
                }
            } else {
                let use_wide_text =
                    unicode_safe_text && is_character_logical_type(&column.logical_type);
                let text = if use_wide_text {
                    fetch_wide_text_column(&mut row, col_num).with_context(|| {
                        format!(
                            "Failed to stream wide text data for column '{}' in table '{}'",
                            column.name, source_qualified_table
                        )
                    })?
                } else {
                    fetch_text_column(&mut row, col_num).with_context(|| {
                        format!(
                            "Failed to stream text data for column '{}' in table '{}'",
                            column.name, source_qualified_table
                        )
                    })?
                };
                if let Some(text) = text {
                    values.push(parse_dm8_value(&column.logical_type, Some(text)));
                } else {
                    values.push(CanonicalValue::Null);
                }
            }
        }

        if !large_blobs.is_empty() {
            // Flush pending normal batch first
            if !batch_rows.is_empty() {
                renderer.write_insert_batch(writer, target_table, &batch_rows)?;
                batch_rows.clear();
            }
            // Delegate to renderer's LOB handler
            renderer.write_lob_insert(writer, target_table, &values, &large_blobs)?;
        } else {
            batch_rows.push(CanonicalRow { values });
        }
        total_rows += 1;

        if batch_rows.len() >= effective_batch_size {
            renderer.write_insert_batch(writer, target_table, &batch_rows)?;
            batch_rows.clear();
        }
    }

    if !batch_rows.is_empty() {
        renderer.write_insert_batch(writer, target_table, &batch_rows)?;
    }

    Ok(total_rows)
}

fn fetch_text_column(row: &mut odbc_api::CursorRow<'_>, col_num: u16) -> Result<Option<String>> {
    let bytes = fetch_text_bytes(row, col_num)?;
    Ok(bytes.map(|bytes| decode_driver_text(&bytes)))
}

fn fetch_wide_text_column(
    row: &mut odbc_api::CursorRow<'_>,
    col_num: u16,
) -> Result<Option<String>> {
    let units = fetch_wide_text_units(row, col_num)?;
    Ok(units.map(|units| String::from_utf16_lossy(&units)))
}

fn fetch_wide_text_units(
    row: &mut odbc_api::CursorRow<'_>,
    col_num: u16,
) -> Result<Option<Vec<u16>>> {
    let mut chunk = VarWCharArray::<STREAM_FETCH_CHUNK_U16>::NULL;
    let mut value = Vec::new();

    loop {
        row.get_data(col_num, &mut chunk)?;
        match chunk.as_slice() {
            Some(units) => value.extend_from_slice(units),
            None if value.is_empty() => return Ok(None),
            None => break,
        }

        if chunk.is_complete() {
            break;
        }
    }

    Ok(Some(value))
}

fn fetch_text_bytes(row: &mut odbc_api::CursorRow<'_>, col_num: u16) -> Result<Option<Vec<u8>>> {
    let mut chunk = VarCharArray::<STREAM_FETCH_CHUNK_BYTES>::NULL;
    let mut value = Vec::new();

    loop {
        row.get_data(col_num, &mut chunk)?;
        match chunk.as_bytes() {
            Some(bytes) => value.extend_from_slice(bytes),
            None if value.is_empty() => return Ok(None),
            None => break,
        }

        if chunk.is_complete() {
            break;
        }
    }

    Ok(Some(value))
}

fn fetch_binary_column(row: &mut odbc_api::CursorRow<'_>, col_num: u16) -> Result<Option<Vec<u8>>> {
    let mut chunk = VarBinaryArray::<STREAM_FETCH_CHUNK_BYTES>::NULL;
    let mut value = Vec::new();

    loop {
        row.get_data(col_num, &mut chunk)?;
        match chunk.as_bytes() {
            Some(bytes) => value.extend_from_slice(bytes),
            None if value.is_empty() => return Ok(None),
            None => break,
        }

        if chunk.is_complete() {
            break;
        }
    }

    Ok(Some(value))
}

fn decode_driver_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GB18030.decode(bytes).0.into_owned(),
    }
}

fn panic_payload_to_string(panic: Box<dyn Any + Send>) -> String {
    let payload = &*panic;
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Parse a raw DM8 cell value into a CanonicalValue based on logical type.
pub fn parse_dm8_value(logical_type: &LogicalType, raw: Option<String>) -> CanonicalValue {
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
                let is_true = s.trim().eq_ignore_ascii_case("Y")
                    || s.trim() == "1"
                    || s.trim().eq_ignore_ascii_case("TRUE")
                    || s.trim().eq_ignore_ascii_case("T");
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

pub fn parse_hex_bytes(hex_str: &str) -> Option<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Column, TableDetails};

    fn table_details_with_column_type(data_type: &str) -> TableDetails {
        TableDetails {
            name: "DICT_TABLE".to_string(),
            comment: None,
            columns: vec![Column {
                name: "LABEL".to_string(),
                data_type: data_type.to_string(),
                length: Some(255),
                precision: None,
                scale: None,
                char_semantics: Some("C".to_string()),
                nullable: true,
                comment: None,
                default_value: None,
                identity: false,
                identity_start: None,
                identity_increment: None,
            }],
            primary_keys: vec![],
            indexes: vec![],
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
            triggers: vec![],
        }
    }

    #[test]
    fn parse_dm8_value_integer() {
        assert_eq!(
            parse_dm8_value(&LogicalType::Integer, Some("42".to_string())),
            CanonicalValue::Integer(42)
        );
    }

    #[test]
    fn parse_dm8_value_boolean_case_insensitive() {
        assert_eq!(
            parse_dm8_value(&LogicalType::Boolean, Some("true".to_string())),
            CanonicalValue::Boolean(true)
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
    fn parse_dm8_value_null_on_empty() {
        assert_eq!(
            parse_dm8_value(&LogicalType::Integer, Some("".to_string())),
            CanonicalValue::Null
        );
        assert_eq!(
            parse_dm8_value(&LogicalType::String, Some("".to_string())),
            CanonicalValue::String(String::new())
        );
    }

    #[test]
    fn parse_hex_bytes_works() {
        assert_eq!(parse_hex_bytes("0xDEAD"), Some(vec![0xDE, 0xAD]));
        assert_eq!(parse_hex_bytes(""), Some(vec![]));
        assert_eq!(parse_hex_bytes("ABC"), None); // odd length
    }

    #[test]
    fn unicode_safe_text_forces_rowwise_export_for_character_tables() {
        let details = table_details_with_column_type("VARCHAR2");
        let table = canonical_table_from_details(&details);

        assert!(should_use_rowwise_export(&details, &table, true));
        assert!(!should_use_rowwise_export(&details, &table, false));
    }

    #[test]
    fn unicode_safe_text_keeps_numeric_tables_on_batch_path() {
        let details = table_details_with_column_type("NUMBER");
        let table = canonical_table_from_details(&details);

        assert!(!should_use_rowwise_export(&details, &table, true));
    }
}
