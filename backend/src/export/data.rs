use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::{
    buffers::TextRowSet,
    parameter::{VarBinaryArray, VarCharArray},
    Connection, Cursor,
};

use crate::db::schema::{decode_cell, fetch_row_count, fetch_sequences, get_tables_details_batch};
use crate::models::{TableDetails, TableIdentifier};

const STREAM_FETCH_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy)]
pub struct TableDataExportSpec<'a> {
    pub source_schema: &'a str,
    pub target_schema: &'a str,
    pub table: &'a str,
    pub table_details: &'a TableDetails,
    pub batch_size: usize,
    pub compat: TriggerTerminator,
}

#[derive(Clone, Copy)]
pub struct SchemaDataExportSpec<'a> {
    pub source_schema: &'a str,
    pub target_schema: &'a str,
    pub tables: &'a [TableIdentifier],
    pub output_path: &'a Path,
    pub batch_size: usize,
    pub include_row_counts: bool,
    pub compat: TriggerTerminator,
}

pub fn export_table_data(
    connection: &Connection<'_>,
    writer: &mut impl Write,
    spec: TableDataExportSpec<'_>,
) -> Result<usize> {
    // Tables with LOB columns (BLOB/CLOB/TEXT etc.) use unbuffered row-by-row
    // fetching to avoid the 8KB TextRowSet truncation limit.
    if has_lob_columns(spec.table_details) {
        return export_table_data_rowwise(connection, writer, spec);
    }

    let TableDataExportSpec {
        source_schema,
        target_schema,
        table,
        table_details,
        batch_size,
        ..
    } = spec;

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

    // Pre-compute the column list string once instead of rebuilding each batch
    let columns_str = column_idents.join(", ");

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

    let mut batch: Vec<String> = Vec::with_capacity(batch_size);
    // Reuse a single Vec<String> across rows to reduce per-row allocations
    let mut values: Vec<String> = Vec::with_capacity(table_details.columns.len());
    let mut row_count = 0;
    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    while let Some(batch_result) = row_set_cursor.fetch()? {
        for row_index in 0..batch_result.num_rows() {
            values.clear();

            for (col_index, column) in table_details.columns.iter().enumerate() {
                let formatted_value = match decode_cell(batch_result, col_index, row_index) {
                    None => "NULL".to_string(),
                    Some(v) => format_literal(&column.data_type, &v),
                };

                values.push(formatted_value);
            }

            batch.push(format!("({})", values.join(", ")));
            row_count += 1;

            if batch.len() >= batch_size {
                write_batch(writer, &target_ident, &columns_str, &batch)?;
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        write_batch(writer, &target_ident, &columns_str, &batch)?;
    }

    tracing::info!(
        "Exported {} rows from {}",
        row_count,
        source_qualified_table
    );
    Ok(row_count)
}

/// Row-by-row export for tables containing LOB (BLOB/CLOB) columns.
///
/// Uses `cursor.next_row()` + chunked `get_data()` to avoid `TextRowSet`
/// truncation and to bypass buggy ODBC length indicators for large values.
fn export_table_data_rowwise(
    connection: &Connection<'_>,
    writer: &mut impl Write,
    spec: TableDataExportSpec<'_>,
) -> Result<usize> {
    let TableDataExportSpec {
        source_schema,
        target_schema,
        table,
        table_details,
        batch_size,
        compat,
    } = spec;

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
    let columns_str = column_idents.join(", ");
    let select_columns = column_idents.join(", ");
    let query = format!("SELECT {} FROM {}", select_columns, source_ident);

    tracing::debug!(
        "Using row-by-row export for {} (has LOB columns)",
        source_qualified_table
    );

    let mut cursor = match connection.execute(&query, ())? {
        Some(cursor) => cursor,
        None => {
            tracing::info!("No data to export for table {}", source_qualified_table);
            return Ok(0);
        }
    };

    let mut batch: Vec<String> = Vec::with_capacity(batch_size);
    let mut row_count = 0;

    while let Some(mut row) = cursor.next_row()? {
        let mut values = Vec::with_capacity(table_details.columns.len());
        // Track large binary columns that need DBMS_LOB treatment
        let mut large_blobs: Vec<(usize, Vec<u8>)> = Vec::new();

        for (col_index, column) in table_details.columns.iter().enumerate() {
            let col_num = (col_index + 1) as u16; // ODBC columns are 1-indexed

            if is_binary_type(&column.data_type) {
                // Binary/BLOB: fetch raw bytes, convert to hex
                let binary = fetch_binary_column(&mut row, col_num).with_context(|| {
                    format!(
                        "Failed to stream binary data for column '{}' in table '{}'",
                        column.name, source_qualified_table
                    )
                })?;
                if let Some(binary) = binary {
                    if binary.len() > HEXTORAW_MAX_BYTES {
                        // Large BLOB: placeholder now, will use DBMS_LOB block
                        large_blobs.push((col_index, binary));
                        values.push("NULL".to_string());
                    } else {
                        values.push(format_hextoraw(&binary));
                    }
                } else {
                    values.push("NULL".to_string());
                }
            } else {
                // All other types: fetch as text
                let text = fetch_text_column(&mut row, col_num).with_context(|| {
                    format!(
                        "Failed to stream text data for column '{}' in table '{}'",
                        column.name, source_qualified_table
                    )
                })?;
                if let Some(text) = text {
                    values.push(format_literal(&column.data_type, &text));
                } else {
                    values.push("NULL".to_string());
                }
            }
        }

        if !large_blobs.is_empty() {
            // Flush any pending normal batch first
            if !batch.is_empty() {
                write_batch(writer, &target_ident, &columns_str, &batch)?;
                batch.clear();
            }
            // Write PL/SQL DBMS_LOB block for this row
            write_lob_insert_block(
                writer,
                &target_ident,
                &columns_str,
                &values,
                &large_blobs,
                compat,
            )?;
        } else {
            batch.push(format!("({})", values.join(", ")));
            if batch.len() >= batch_size {
                write_batch(writer, &target_ident, &columns_str, &batch)?;
                batch.clear();
            }
        }
        row_count += 1;
    }

    if !batch.is_empty() {
        write_batch(writer, &target_ident, &columns_str, &batch)?;
    }

    tracing::info!(
        "Exported {} rows (row-by-row/LOB) from {}",
        row_count,
        source_qualified_table
    );
    Ok(row_count)
}

fn fetch_text_column(row: &mut odbc_api::CursorRow<'_>, col_num: u16) -> Result<Option<String>> {
    let bytes = fetch_text_bytes(row, col_num)?;
    Ok(bytes.map(|bytes| decode_driver_text(&bytes)))
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

/// Check if a table has any LOB (large object) columns that need
/// unbuffered row-by-row fetching to avoid truncation.
pub fn has_lob_columns(table_details: &TableDetails) -> bool {
    table_details
        .columns
        .iter()
        .any(|col| is_lob_type(&col.data_type))
}

/// Types that can hold arbitrarily large data and would be truncated
/// by the fixed-size TextRowSet buffer (8KB limit).
pub fn is_lob_type(data_type: &str) -> bool {
    matches!(
        data_type.to_uppercase().as_str(),
        "BLOB"
            | "CLOB"
            | "NCLOB"
            | "TEXT"
            | "NTEXT"
            | "IMAGE"
            | "LONG"
            | "LONG RAW"
            | "LONGVARCHAR"
            | "LONGVARBINARY"
            | "LONG VARCHAR"
            | "LONG VARBINARY"
    )
}

/// Convert a byte slice to an uppercase hex string.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(hex, "{:02X}", b);
    }
    hex
}

/// Maximum binary bytes that fit in a single HEXTORAW() call or RAW concatenation.
/// DM8 limits RAW/HEXTORAW results to 16383 bytes (32766 hex chars).
const HEXTORAW_MAX_BYTES: usize = 16383;

/// Maximum hex characters per DBMS_LOB.APPEND chunk (must stay under 32766).
const LOB_APPEND_HEX_CHUNK: usize = 32000;

/// Format binary data as a HEXTORAW expression for inline use in VALUES.
/// Only works for data <= 16383 bytes. Caller must use PL/SQL DBMS_LOB
/// approach for larger data.
fn format_hextoraw(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "HEXTORAW('')".to_string();
    }
    format!("HEXTORAW('{}')", bytes_to_hex(bytes))
}

use crate::export::ddl::TriggerTerminator;

/// Write a PL/SQL block that uses DBMS_LOB.APPEND to insert a row
/// with large BLOB data that exceeds HEXTORAW's 16383-byte limit.
///
/// All modes emit an anonymous `DECLARE...BEGIN...END; /` block.
/// For DataGrip/DataGripScript modes, the block is wrapped with
/// `--@delimiter /` and `--@delimiter ;` directives so that DataGrip's
/// "Run Script" mode does not split at internal semicolons.
fn write_lob_insert_block(
    writer: &mut impl Write,
    target_ident: &str,
    columns_str: &str,
    column_values: &[String],
    large_blobs: &[(usize, Vec<u8>)], // (column_index, raw_bytes)
    compat: TriggerTerminator,
) -> Result<()> {
    // Build VALUES with v_blob placeholders substituted
    let mut final_values = column_values.to_vec();
    for (i, (col_idx, _)) in large_blobs.iter().enumerate() {
        final_values[*col_idx] = format!("v_blob{}", i);
    }

    let use_delimiter_directive = matches!(
        compat,
        TriggerTerminator::DataGrip | TriggerTerminator::DataGripScript
    );

    if use_delimiter_directive {
        writeln!(writer, "--@delimiter /")?;
    }

    writeln!(writer, "DECLARE")?;

    // Variable declarations
    for (i, _) in large_blobs.iter().enumerate() {
        writeln!(writer, "  v_blob{} BLOB;", i)?;
    }
    writeln!(writer, "BEGIN")?;

    // DBMS_LOB.CREATETEMPORARY + APPEND chunks
    for (i, (_, bytes)) in large_blobs.iter().enumerate() {
        writeln!(writer, "  DBMS_LOB.CREATETEMPORARY(v_blob{}, TRUE);", i)?;
        let hex = bytes_to_hex(bytes);
        for chunk in hex.as_bytes().chunks(LOB_APPEND_HEX_CHUNK) {
            let s = std::str::from_utf8(chunk).unwrap();
            writeln!(writer, "  DBMS_LOB.APPEND(v_blob{}, HEXTORAW('{}'));", i, s)?;
        }
    }

    // INSERT statement
    writeln!(
        writer,
        "  INSERT INTO {} ({}) VALUES ({});",
        target_ident,
        columns_str,
        final_values.join(", ")
    )?;

    // Cleanup
    for (i, _) in large_blobs.iter().enumerate() {
        writeln!(writer, "  DBMS_LOB.FREETEMPORARY(v_blob{});", i)?;
    }
    writeln!(writer, "END;")?;
    writeln!(writer, "/")?;

    if use_delimiter_directive {
        writeln!(writer, "--@delimiter ;")?;
    }
    Ok(())
}

pub fn export_schema_data(
    connection: &Connection<'_>,
    spec: SchemaDataExportSpec<'_>,
) -> Result<usize> {
    let SchemaDataExportSpec {
        source_schema,
        target_schema,
        tables,
        output_path,
        batch_size,
        include_row_counts,
        compat,
    } = spec;

    let source_schema_upper = source_schema.to_uppercase();
    let target_schema_upper = target_schema.to_uppercase();
    let all_sequences = fetch_sequences(connection, &source_schema_upper).unwrap_or_default();

    // Pre-fetch table details（批量查询，减少 ODBC 往返次数）
    let table_names: Vec<String> = tables.iter().map(|t| t.name.clone()).collect();
    let table_details_cache: Vec<crate::models::TableDetails> =
        get_tables_details_batch(connection, &source_schema_upper, &table_names)
            .context("Failed to batch-fetch table metadata")?;

    // Compute FK-aware table ordering (parents before children for INSERT)
    let insert_order = topological_sort_by_fk(tables, &table_details_cache);
    // Reverse for TRUNCATE order (children before parents)
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    let sequences =
        crate::export::ddl::filter_sequences_for_tables(&all_sequences, &table_details_cache);

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                output_path.display()
            )
        })?;
    }

    let file = File::create(output_path).with_context(|| {
        format!(
            "Failed to create data export file at {}",
            output_path.display()
        )
    })?;
    // Use a large write buffer to reduce syscall overhead for big exports
    let mut writer = BufWriter::with_capacity(1024 * 1024, file);

    // Pre-compute row counts for header (optional), indexed by original position
    let mut total_rows: i64 = 0;
    let mut table_row_counts: Vec<Option<i64>> = vec![None; tables.len()];
    if include_row_counts {
        for (i, table_id) in tables.iter().enumerate() {
            if let Ok(cnt) = fetch_row_count(connection, &source_schema_upper, &table_id.name) {
                total_rows += cnt;
                table_row_counts[i] = Some(cnt);
            }
        }
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    write_data_export_header(
        &mut writer,
        tables.len(),
        include_row_counts,
        total_rows,
        &timestamp,
        !sequences.is_empty(),
    )?;

    if !sequences.is_empty() {
        writeln!(
            writer,
            "-- 重置序列（DM8 使用 CURRENT VALUE，而非 RESTART WITH）"
        )?;
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

    // Collect all FK constraint names per table for disable/enable
    let fk_constraints: Vec<(String, Vec<String>)> = table_details_cache
        .iter()
        .enumerate()
        .filter_map(|(idx, details)| {
            let fk_names: Vec<String> = details
                .foreign_keys
                .iter()
                .map(|fk| fk.name.clone())
                .collect();
            if fk_names.is_empty() {
                None
            } else {
                let table_upper = tables[idx].name.to_uppercase();
                let qualified =
                    quote_identifier(&format!("{}.{}", target_schema_upper, table_upper));
                Some((qualified, fk_names))
            }
        })
        .collect();

    // Phase 0: Disable all FK constraints to avoid ordering issues
    if !fk_constraints.is_empty() {
        writeln!(writer, "-- ========================================")?;
        writeln!(
            writer,
            "-- 阶段零：禁用外键约束（避免插入顺序导致的约束违反）"
        )?;
        writeln!(writer, "-- ========================================")?;
        writeln!(writer)?;
        for (qualified_table, fk_names) in &fk_constraints {
            for fk_name in fk_names {
                writeln!(
                    writer,
                    "ALTER TABLE {} DISABLE CONSTRAINT \"{}\";",
                    qualified_table, fk_name
                )?;
            }
        }
        writeln!(writer)?;
    }

    // Phase 1: TRUNCATE all tables (children first, then parents)
    writeln!(writer, "-- ========================================")?;
    writeln!(
        writer,
        "-- 阶段一：清空表数据（按外键依赖反序，先子表后父表）"
    )?;
    writeln!(writer, "-- ========================================")?;
    writeln!(writer)?;
    for &idx in &truncate_order {
        let table_upper = tables[idx].name.to_uppercase();
        let qualified = quote_identifier(&format!("{}.{}", target_schema_upper, table_upper));
        writeln!(writer, "TRUNCATE TABLE {};", qualified)?;
    }
    writeln!(writer)?;

    // Phase 2: INSERT data (parents first, then children)
    writeln!(writer, "-- ========================================")?;
    writeln!(
        writer,
        "-- 阶段二：插入数据（按外键依赖顺序，先父表后子表）"
    )?;
    writeln!(writer, "-- ========================================")?;
    writeln!(writer)?;

    let mut exported_total: usize = 0;

    for (seq, &idx) in insert_order.iter().enumerate() {
        if seq > 0 {
            writeln!(writer)?;
        }

        let table_upper = tables[idx].name.to_uppercase();
        let table_details = &table_details_cache[idx];
        let has_identity = table_details.columns.iter().any(|col| col.identity);
        let expected_rows = table_row_counts[idx];

        writeln!(
            writer,
            "-- 表数据：{}.{}{}",
            target_schema_upper,
            table_upper,
            expected_rows
                .map(|c| format!("（{} 行）", c))
                .unwrap_or_else(|| "（行数未知）".to_string())
        )?;
        let qualified = quote_identifier(&format!("{}.{}", target_schema_upper, table_upper));

        if has_identity {
            write_identity_insert(&mut writer, &qualified, true)?;
        }

        let count = export_table_data(
            connection,
            &mut writer,
            TableDataExportSpec {
                source_schema: &source_schema_upper,
                target_schema: &target_schema_upper,
                table: &tables[idx].name,
                table_details,
                batch_size,
                compat,
            },
        )
        .with_context(|| format!("Failed to export data for table '{}'", tables[idx].name))?;

        if has_identity {
            write_identity_insert(&mut writer, &qualified, false)?;
        }

        exported_total += count;
    }

    // Phase 3: Re-enable FK constraints
    if !fk_constraints.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- ========================================")?;
        writeln!(writer, "-- 阶段三：重新启用外键约束")?;
        writeln!(writer, "-- ========================================")?;
        writeln!(writer)?;
        for (qualified_table, fk_names) in &fk_constraints {
            for fk_name in fk_names {
                writeln!(
                    writer,
                    "ALTER TABLE {} ENABLE CONSTRAINT \"{}\";",
                    qualified_table, fk_name
                )?;
            }
        }
    }

    writer
        .flush()
        .context("Failed to flush data export to disk")?;
    Ok(exported_total)
}

/// Generic FK topological sort: accepts table names + per-table FK lists,
/// returns insert-order indices (parents before children).
///
/// If cycles exist, remaining tables are appended in their original order.
pub fn topological_sort_by_foreign_keys(
    table_names: &[String],
    table_fks: &[Vec<crate::models::ForeignKey>],
) -> Vec<usize> {
    let n = table_names.len();
    if n <= 1 {
        return (0..n).collect();
    }

    // Build name → index map (uppercase table names)
    let name_to_idx: HashMap<String, usize> = table_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.to_uppercase(), i))
        .collect();

    // Collect unique directed edges: parent_idx → child_idx
    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for (child_idx, fks) in table_fks.iter().enumerate() {
        for fk in fks {
            // referenced_table is "SCHEMA.TABLE" format
            let ref_table_name = fk
                .referenced_table
                .rsplit('.')
                .next()
                .unwrap_or(&fk.referenced_table)
                .to_uppercase();

            // Only consider dependencies within the selected table set
            if let Some(&parent_idx) = name_to_idx.get(&ref_table_name) {
                if parent_idx != child_idx {
                    edges.insert((parent_idx, child_idx));
                }
            }
        }
    }

    if edges.is_empty() {
        // No FK dependencies among selected tables — keep original order
        return (0..n).collect();
    }

    // Build adjacency list and in-degree counts
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![vec![]; n];
    for &(parent, child) in &edges {
        in_degree[child] += 1;
        dependents[parent].push(child);
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, degree) in in_degree.iter().enumerate() {
        if *degree == 0 {
            queue.push_back(i);
        }
    }

    let mut sorted = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        sorted.push(idx);
        for &child in &dependents[idx] {
            in_degree[child] -= 1;
            if in_degree[child] == 0 {
                queue.push_back(child);
            }
        }
    }

    // If there are cycles, append remaining tables in original order and warn
    if sorted.len() < n {
        let in_sorted: HashSet<usize> = sorted.iter().copied().collect();
        let mut cyclic_names = Vec::new();
        for (i, table_name) in table_names.iter().enumerate() {
            if !in_sorted.contains(&i) {
                cyclic_names.push(table_name.clone());
                sorted.push(i);
            }
        }
        tracing::warn!(
            "Circular FK dependencies detected among tables: {:?}. \
             These tables are appended in original order.",
            cyclic_names
        );
    }

    tracing::debug!(
        "FK-aware table order: {:?}",
        sorted
            .iter()
            .map(|&i| table_names[i].as_str())
            .collect::<Vec<_>>()
    );

    sorted
}

/// FK-aware topological sort that returns **layers** instead of a flat list.
///
/// Each layer is a `Vec<usize>` of table indices that share the same depth in
/// the FK dependency graph. Tables within the same layer have no mutual FK
/// dependencies and can be safely exported in parallel.
///
/// - Layer 0: root tables (no FK dependencies within the selected set)
/// - Layer 1: tables that only depend on Layer 0 tables
/// - Layer N: tables that only depend on Layer 0..N-1 tables
///
/// If cycles exist, remaining tables are placed in a final catch-all layer.
pub fn topological_sort_into_layers(
    table_names: &[String],
    table_fks: &[Vec<crate::models::ForeignKey>],
) -> Vec<Vec<usize>> {
    let n = table_names.len();
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![vec![0]];
    }

    // Build name → index map (uppercase table names)
    let name_to_idx: HashMap<String, usize> = table_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.to_uppercase(), i))
        .collect();

    // Collect unique directed edges: parent_idx → child_idx
    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for (child_idx, fks) in table_fks.iter().enumerate() {
        for fk in fks {
            let ref_table_name = fk
                .referenced_table
                .rsplit('.')
                .next()
                .unwrap_or(&fk.referenced_table)
                .to_uppercase();
            if let Some(&parent_idx) = name_to_idx.get(&ref_table_name) {
                if parent_idx != child_idx {
                    edges.insert((parent_idx, child_idx));
                }
            }
        }
    }

    if edges.is_empty() {
        // No FK dependencies — all tables in a single layer
        return vec![(0..n).collect()];
    }

    // Build adjacency list and in-degree counts
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![vec![]; n];
    for &(parent, child) in &edges {
        in_degree[child] += 1;
        dependents[parent].push(child);
    }

    // Modified Kahn's algorithm: process one full layer at a time
    let mut current_layer: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut layers: Vec<Vec<usize>> = Vec::new();
    let mut processed = 0usize;

    while !current_layer.is_empty() {
        let mut next_layer: Vec<usize> = Vec::new();
        for &idx in &current_layer {
            for &child in &dependents[idx] {
                in_degree[child] -= 1;
                if in_degree[child] == 0 {
                    next_layer.push(child);
                }
            }
        }
        processed += current_layer.len();
        layers.push(current_layer);
        current_layer = next_layer;
    }

    // Handle cycles: append remaining tables as a final layer
    if processed < n {
        let in_layers: HashSet<usize> = layers.iter().flatten().copied().collect();
        let cyclic: Vec<usize> = (0..n).filter(|i| !in_layers.contains(i)).collect();
        let cyclic_names: Vec<&str> = cyclic.iter().map(|&i| table_names[i].as_str()).collect();
        tracing::warn!(
            "Circular FK dependencies detected among tables: {:?}. \
             Placed in final catch-all layer.",
            cyclic_names
        );
        layers.push(cyclic);
    }

    tracing::debug!(
        "FK layer sort: {} layers, tables per layer: {:?}",
        layers.len(),
        layers.iter().map(|l| l.len()).collect::<Vec<_>>()
    );

    layers
}

/// Topological sort of tables based on foreign key dependencies.
///
/// Returns indices into the original `tables` array, ordered so that
/// parent tables (referenced by FKs) come before child tables (that have FKs).
/// If cycles exist, remaining tables are appended in their original order.
fn topological_sort_by_fk(
    tables: &[TableIdentifier],
    table_details: &[TableDetails],
) -> Vec<usize> {
    let names: Vec<String> = tables.iter().map(|t| t.name.clone()).collect();
    let fks: Vec<Vec<crate::models::ForeignKey>> = table_details
        .iter()
        .map(|d| d.foreign_keys.clone())
        .collect();
    topological_sort_by_foreign_keys(&names, &fks)
}

fn write_data_export_header(
    writer: &mut impl Write,
    table_count: usize,
    include_row_counts: bool,
    total_rows: i64,
    timestamp: &str,
    has_sequences: bool,
) -> Result<()> {
    writeln!(writer, "-- DM8 数据导出脚本")?;
    writeln!(writer, "-- 表数量: {}", table_count)?;
    if include_row_counts {
        writeln!(writer, "-- 预计总行数: {}", total_rows)?;
    } else {
        writeln!(writer, "-- 预计总行数: 已跳过（按请求）")?;
    }
    writeln!(writer, "-- 生成时间: {}", timestamp)?;
    writeln!(writer, "-- 警告: 本脚本会先 TRUNCATE 表，再执行数据插入。")?;
    if has_sequences {
        writeln!(writer, "-- 说明: 插入前会将序列重置到 START 值。")?;
    }
    writeln!(writer)?;
    Ok(())
}

fn write_batch(
    writer: &mut impl Write,
    table: &str,
    columns_str: &str,
    batch: &[String],
) -> Result<()> {
    // Write INSERT header once
    writeln!(writer, "INSERT INTO {} ({}) VALUES", table, columns_str)?;
    // Stream each row directly to avoid building one giant intermediate String
    let last = batch.len().saturating_sub(1);
    for (i, row) in batch.iter().enumerate() {
        if i == last {
            writeln!(writer, "{};", row)?;
        } else {
            writeln!(writer, "{},", row)?;
        }
    }
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
        "NUMBER"
            | "INTEGER"
            | "INT"
            | "SMALLINT"
            | "BIGINT"
            | "DECIMAL"
            | "NUMERIC"
            | "FLOAT"
            | "DOUBLE"
            | "REAL"
    )
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "''")
}

/// Decode a hex string (e.g. "4F2A") into raw bytes.
/// Used by format_literal's binary path where TextRowSet returns hex text.
fn hex_str_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            if pair.len() == 2 {
                let hi = (pair[0] as char).to_digit(16)?;
                let lo = (pair[1] as char).to_digit(16)?;
                Some((hi * 16 + lo) as u8)
            } else {
                // Odd trailing nibble — treat as high nibble
                let hi = (pair[0] as char).to_digit(16)?;
                Some((hi * 16) as u8)
            }
        })
        .collect()
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
    matches!(
        dt.to_uppercase().as_str(),
        "RAW" | "BINARY" | "VARBINARY" | "BLOB"
    )
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
    if let Some(pos) = normalized.rfind(['+', '-']) {
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
        let bytes = hex_str_to_bytes(trimmed);
        return format_hextoraw(&bytes);
    }
    if is_date_type(&upper) {
        // Choose format based on actual value content
        let format_str = if raw.contains(':') {
            "YYYY-MM-DD HH24:MI:SS"
        } else {
            "YYYY-MM-DD"
        };
        return format!("TO_DATE('{}','{}')", escape_single_quotes(raw), format_str);
    }
    if is_timestamp_type(&upper) {
        // Normalize ISO 8601 format to DM8-compatible format
        let normalized = normalize_iso8601_timestamp(raw.trim());

        // Detect timezone offset (+HH:MM or -HH:MM after time part)
        let has_tz = has_timezone_offset(&normalized);

        // Extract main part (without timezone) for format string analysis
        let main_part = if has_tz {
            normalized
                .rfind(['+', '-'])
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
    if let Some(pos) = s.rfind(['+', '-']) {
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

#[cfg(test)]
mod tests {
    use super::topological_sort_by_fk;
    use crate::models::{ForeignKey, TableDetails, TableIdentifier};

    fn make_table_id(name: &str) -> TableIdentifier {
        TableIdentifier {
            schema: "S".to_string(),
            name: name.to_string(),
        }
    }

    fn make_details(name: &str, fks: Vec<ForeignKey>) -> TableDetails {
        TableDetails {
            name: name.to_uppercase(),
            comment: None,
            columns: vec![],
            primary_keys: vec![],
            indexes: vec![],
            unique_constraints: vec![],
            foreign_keys: fks,
            check_constraints: vec![],
            triggers: vec![],
        }
    }

    fn make_fk(ref_table: &str) -> ForeignKey {
        ForeignKey {
            name: format!("fk_{}", ref_table),
            columns: vec!["id".to_string()],
            referenced_table: format!("S.{}", ref_table.to_uppercase()),
            referenced_columns: vec!["id".to_string()],
            delete_rule: None,
            update_rule: None,
        }
    }

    #[test]
    fn topo_sort_parents_before_children() {
        // CHILD -> PARENT (FK)
        let tables = vec![make_table_id("CHILD"), make_table_id("PARENT")];
        let details = vec![
            make_details("CHILD", vec![make_fk("PARENT")]),
            make_details("PARENT", vec![]),
        ];

        let order = topological_sort_by_fk(&tables, &details);
        let parent_pos = order.iter().position(|&i| i == 1).unwrap();
        let child_pos = order.iter().position(|&i| i == 0).unwrap();
        assert!(
            parent_pos < child_pos,
            "PARENT should come before CHILD in insert order"
        );
    }

    #[test]
    fn topo_sort_no_fks_preserves_order() {
        let tables = vec![make_table_id("A"), make_table_id("B"), make_table_id("C")];
        let details = vec![
            make_details("A", vec![]),
            make_details("B", vec![]),
            make_details("C", vec![]),
        ];

        let order = topological_sort_by_fk(&tables, &details);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn topo_sort_chain_a_b_c() {
        // C -> B -> A
        let tables = vec![make_table_id("A"), make_table_id("B"), make_table_id("C")];
        let details = vec![
            make_details("A", vec![]),
            make_details("B", vec![make_fk("A")]),
            make_details("C", vec![make_fk("B")]),
        ];

        let order = topological_sort_by_fk(&tables, &details);
        let a_pos = order.iter().position(|&i| i == 0).unwrap();
        let b_pos = order.iter().position(|&i| i == 1).unwrap();
        let c_pos = order.iter().position(|&i| i == 2).unwrap();
        assert!(a_pos < b_pos, "A before B");
        assert!(b_pos < c_pos, "B before C");
    }

    #[test]
    fn topo_sort_handles_cycle() {
        // A -> B -> A (cycle)
        let tables = vec![make_table_id("A"), make_table_id("B")];
        let details = vec![
            make_details("A", vec![make_fk("B")]),
            make_details("B", vec![make_fk("A")]),
        ];

        let order = topological_sort_by_fk(&tables, &details);
        // Should still return all tables (cycle fallback)
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn topo_sort_ignores_external_fk() {
        // CHILD references EXTERNAL (not in selected tables)
        let tables = vec![make_table_id("CHILD")];
        let details = vec![make_details("CHILD", vec![make_fk("EXTERNAL")])];

        let order = topological_sort_by_fk(&tables, &details);
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn write_data_export_header_uses_chinese_comments() {
        let mut buf = Vec::new();
        super::write_data_export_header(&mut buf, 3, true, 128, "2026-02-06 10:30:00", true)
            .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        assert!(rendered.contains("-- DM8 数据导出脚本"));
        assert!(rendered.contains("-- 表数量: 3"));
        assert!(rendered.contains("-- 预计总行数: 128"));
        assert!(rendered.contains("-- 生成时间: 2026-02-06 10:30:00"));
        assert!(rendered.contains("-- 警告: 本脚本会先 TRUNCATE 表，再执行数据插入。"));
        assert!(rendered.contains("-- 说明: 插入前会将序列重置到 START 值。"));
        assert!(!rendered.contains("DM8 Data Export"));
    }

    #[test]
    fn write_lob_insert_block_datagrip_uses_delimiter_directive() {
        use super::write_lob_insert_block;
        use crate::export::ddl::TriggerTerminator;

        let mut buf = Vec::new();
        // Simulate a row with one large blob column (col index 1)
        let values = vec!["'hello'".to_string(), "NULL".to_string()];
        let large_blobs = vec![(1usize, vec![0xABu8; 100])];

        write_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"COL1\", \"COL2\"",
            &values,
            &large_blobs,
            TriggerTerminator::DataGrip,
        )
        .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        // Should have --@delimiter directives
        assert!(
            rendered.starts_with("--@delimiter /\n"),
            "should start with --@delimiter /"
        );
        assert!(
            rendered.trim_end().ends_with("--@delimiter ;"),
            "should end with --@delimiter ;"
        );
        // Should use anonymous DECLARE block, NOT CREATE PROCEDURE
        assert!(rendered.contains("DECLARE"), "should use DECLARE block");
        assert!(
            !rendered.contains("CREATE OR REPLACE PROCEDURE"),
            "should NOT use CREATE PROCEDURE"
        );
        assert!(
            !rendered.contains("_TMP_LOB_"),
            "should NOT have temp procedure name"
        );
        // Should have proper PL/SQL structure
        assert!(rendered.contains("DBMS_LOB.CREATETEMPORARY(v_blob0, TRUE);"));
        assert!(rendered.contains("DBMS_LOB.APPEND(v_blob0, HEXTORAW("));
        assert!(rendered.contains("INSERT INTO"));
        assert!(rendered.contains("DBMS_LOB.FREETEMPORARY(v_blob0);"));
        assert!(rendered.contains("END;\n/\n"));
    }

    #[test]
    fn write_lob_insert_block_script_mode_no_delimiter_directive() {
        use super::write_lob_insert_block;
        use crate::export::ddl::TriggerTerminator;

        let mut buf = Vec::new();
        let values = vec!["'hello'".to_string(), "NULL".to_string()];
        let large_blobs = vec![(1usize, vec![0xCDu8; 50])];

        write_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"COL1\", \"COL2\"",
            &values,
            &large_blobs,
            TriggerTerminator::Script,
        )
        .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        // Should NOT have --@delimiter directives
        assert!(
            !rendered.contains("--@delimiter"),
            "Script mode should not have --@delimiter"
        );
        // Should use anonymous DECLARE block
        assert!(rendered.contains("DECLARE"));
        assert!(rendered.contains("END;\n/\n"));
    }

    #[test]
    fn write_lob_insert_block_datagrip_script_uses_delimiter_directive() {
        use super::write_lob_insert_block;
        use crate::export::ddl::TriggerTerminator;

        let mut buf = Vec::new();
        let values = vec!["NULL".to_string()];
        let large_blobs = vec![(0usize, vec![0xEFu8; 30])];

        write_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"COL1\"",
            &values,
            &large_blobs,
            TriggerTerminator::DataGripScript,
        )
        .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        assert!(rendered.contains("--@delimiter /"));
        assert!(rendered.contains("--@delimiter ;"));
        assert!(rendered.contains("DECLARE"));
        assert!(!rendered.contains("CREATE OR REPLACE PROCEDURE"));
    }

    // ── topological_sort_into_layers tests ───────────────────────────

    fn make_fk_vec(ref_table: &str) -> Vec<ForeignKey> {
        vec![make_fk(ref_table)]
    }

    #[test]
    fn layers_no_fks_all_in_single_layer() {
        use super::topological_sort_into_layers;
        let names: Vec<String> = vec!["A", "B", "C"].into_iter().map(String::from).collect();
        let fks: Vec<Vec<ForeignKey>> = vec![vec![], vec![], vec![]];
        let layers = topological_sort_into_layers(&names, &fks);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].len(), 3);
    }

    #[test]
    fn layers_chain_produces_sequential_layers() {
        use super::topological_sort_into_layers;
        // A <- B <- C (C depends on B, B depends on A)
        let names: Vec<String> = vec!["A", "B", "C"].into_iter().map(String::from).collect();
        let fks: Vec<Vec<ForeignKey>> = vec![
            vec![],           // A has no FK
            make_fk_vec("A"), // B -> A
            make_fk_vec("B"), // C -> B
        ];
        let layers = topological_sort_into_layers(&names, &fks);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0], vec![0]); // A
        assert_eq!(layers[1], vec![1]); // B
        assert_eq!(layers[2], vec![2]); // C
    }

    #[test]
    fn layers_diamond_two_layers() {
        use super::topological_sort_into_layers;
        // PARENT <- CHILD1, PARENT <- CHILD2 (both children depend on parent)
        let names: Vec<String> = vec!["PARENT", "CHILD1", "CHILD2"]
            .into_iter()
            .map(String::from)
            .collect();
        let fks: Vec<Vec<ForeignKey>> = vec![
            vec![],                // PARENT
            make_fk_vec("PARENT"), // CHILD1 -> PARENT
            make_fk_vec("PARENT"), // CHILD2 -> PARENT
        ];
        let layers = topological_sort_into_layers(&names, &fks);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0], vec![0]); // PARENT in layer 0
                                        // CHILD1 and CHILD2 in layer 1 (parallel-safe)
        assert!(layers[1].contains(&1));
        assert!(layers[1].contains(&2));
    }

    #[test]
    fn layers_cycle_appends_catch_all_layer() {
        use super::topological_sort_into_layers;
        // A -> B, B -> A (cycle), C has no FK
        let names: Vec<String> = vec!["A", "B", "C"].into_iter().map(String::from).collect();
        let fks: Vec<Vec<ForeignKey>> = vec![
            make_fk_vec("B"), // A -> B
            make_fk_vec("A"), // B -> A
            vec![],           // C has no FK
        ];
        let layers = topological_sort_into_layers(&names, &fks);
        // C should be in layer 0 (no deps), A and B in a catch-all layer
        let flat: Vec<usize> = layers.iter().flatten().copied().collect();
        assert_eq!(flat.len(), 3);
        // C should appear before A and B
        let c_pos = flat.iter().position(|&x| x == 2).unwrap();
        let a_pos = flat.iter().position(|&x| x == 0).unwrap();
        let b_pos = flat.iter().position(|&x| x == 1).unwrap();
        assert!(c_pos < a_pos);
        assert!(c_pos < b_pos);
    }
}
