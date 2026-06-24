use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};

use crate::{
    db::shentong as db_shentong,
    dialect::{
        renderer_for, shentong_format_value, shentong_quote_ident, SHENTONG_INLINE_BLOB_MAX_BYTES,
    },
    domain::canonical::{canonical_table_from_details, CanonicalRow, CanonicalValue, LogicalType},
    export::{data::topological_sort_by_foreign_keys, orchestrator::LegacyExportPlan},
    models::{ConnectionConfig, DbType, ForeignKey, TableDetails},
};

pub fn export_shentong_to_shentong_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
) -> Result<()> {
    let conn = db_shentong::open(config)?;
    let renderer = renderer_for(&DbType::Shentong);

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);

    writeln!(
        writer,
        "-- Shentong OSCAR -> Shentong OSCAR DDL export script"
    )?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    for (idx, table_id) in plan.tables.iter().enumerate() {
        let details = inspect_table_details(&conn, &plan.source_schema, &table_id.name)
            .with_context(|| format!("Failed to inspect Shentong table '{}'", table_id))?;
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
        .context("Failed to flush shentong ddl export")?;
    Ok(())
}

pub fn export_shentong_to_shentong_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    let renderer = renderer_for(&DbType::Shentong);

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut total_rows = 0usize;

    writeln!(
        writer,
        "-- Shentong OSCAR -> Shentong OSCAR data export script"
    )?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    // Use a single connection for FK metadata and all data queries
    let conn = db_shentong::open(config)?;
    let owner = plan.source_schema.trim().to_uppercase();

    // Phase 0: Collect FK info for all tables + collect constraint names
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());
    for table_id in &plan.tables {
        let table_upper = table_id.name.trim().to_uppercase();
        let fks = db_shentong::fetch_foreign_keys(&conn, &owner, &table_upper).unwrap_or_default();
        table_names.push(table_id.name.clone());
        table_fks.push(fks);
    }

    // Compute FK-aware ordering
    let insert_order = topological_sort_by_foreign_keys(&table_names, &table_fks);
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    // Phase 0: Disable FK constraints to allow TRUNCATE and out-of-order INSERT
    let has_fks = table_fks.iter().any(|fks| !fks.is_empty());
    if has_fks {
        writeln!(writer, "-- Phase 0: Disable FK constraints")?;
        for (idx, fks) in table_fks.iter().enumerate() {
            let table_id = &plan.tables[idx];
            for fk in fks {
                writeln!(
                    writer,
                    "ALTER TABLE {}.{} DISABLE CONSTRAINT {};",
                    quote_ident(&plan.target_schema),
                    quote_ident(&table_id.name),
                    quote_ident(&fk.name)
                )?;
            }
        }
        writeln!(writer)?;
    }

    // Phase 1: TRUNCATE all tables (children first)
    writeln!(
        writer,
        "-- Phase 1: TRUNCATE tables (children before parents)"
    )?;
    for &idx in &truncate_order {
        let table_id = &plan.tables[idx];
        writeln!(
            writer,
            "TRUNCATE TABLE {}.{};",
            quote_ident(&plan.target_schema),
            quote_ident(&table_id.name)
        )?;
    }
    writeln!(writer)?;

    // Phase 2: INSERT data (parents first) — reuse same connection
    writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;
    for &idx in &insert_order {
        let table_id = &plan.tables[idx];
        let details = inspect_table_details(&conn, &plan.source_schema, &table_id.name)
            .with_context(|| format!("Failed to inspect Shentong table '{}'", table_id))?;

        let source_table = canonical_table_from_details(&details);
        let mut target_table = canonical_table_from_details(&details);
        target_table.name = format!("{}.{}", plan.target_schema, details.name);

        let count = export_table_rows(
            &conn,
            &plan.source_schema,
            &source_table,
            &target_table,
            renderer.as_ref(),
            &mut writer,
            batch_size.max(1),
        )
        .with_context(|| format!("Failed to fetch rows from table '{}'", table_id))?;

        if count > 0 {
            total_rows += count;
            writeln!(writer)?;
        }
    }

    // Phase 3: Re-enable FK constraints
    if has_fks {
        writeln!(writer, "-- Phase 3: Re-enable FK constraints")?;
        for (idx, fks) in table_fks.iter().enumerate() {
            let table_id = &plan.tables[idx];
            for fk in fks {
                writeln!(
                    writer,
                    "ALTER TABLE {}.{} ENABLE CONSTRAINT {};",
                    quote_ident(&plan.target_schema),
                    quote_ident(&table_id.name),
                    quote_ident(&fk.name)
                )?;
            }
        }
    }

    writer
        .flush()
        .context("Failed to flush shentong data export")?;
    Ok(total_rows)
}

/// Inspect table metadata via db_shentong (reuses public functions from db::shentong).
fn inspect_table_details(
    conn: &shentong::Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let owner = schema.trim().to_uppercase();
    let table_name = table.trim().to_uppercase();

    let columns = db_shentong::fetch_columns(conn, &owner, &table_name)?;
    let primary_keys = db_shentong::fetch_primary_keys(conn, &owner, &table_name)?;

    Ok(TableDetails {
        name: table.trim().to_string(),
        comment: None,
        columns,
        primary_keys,
        indexes: vec![],
        unique_constraints: vec![],
        foreign_keys: vec![],
        check_constraints: vec![],
        triggers: vec![],
    })
}

fn export_table_rows(
    conn: &shentong::Connection,
    source_schema: &str,
    source_table: &crate::domain::canonical::CanonicalTable,
    target_table: &crate::domain::canonical::CanonicalTable,
    renderer: &dyn crate::dialect::DialectRenderer,
    writer: &mut impl Write,
    batch_size: usize,
) -> Result<usize> {
    if source_table.columns.is_empty() {
        return Ok(0);
    }

    // Shentong OSCAR ACI: set search_path so unqualified table names resolve
    let set_sql = format!("SET search_path TO {}, public", quote_ident(source_schema));
    tracing::debug!(sql = %set_sql, "Setting Shentong search_path");
    match conn.execute(&set_sql, &[]) {
        Ok(_) => tracing::debug!("search_path set successfully"),
        Err(e) => tracing::warn!(error = ?e, "Failed to set search_path"),
    }

    // Build SELECT with properly quoted column names
    let selected_columns = source_table
        .columns
        .iter()
        .map(|col| quote_ident(&col.name))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "SELECT {} FROM {}",
        selected_columns,
        quote_ident(&source_table.name)
    );

    tracing::debug!(sql = %sql, "Shentong data export query");

    let query_rows = conn
        .query(&sql, &[])
        .context("Failed to query Shentong table rows")?;

    let logical_types: Vec<_> = source_table
        .columns
        .iter()
        .map(|col| col.logical_type.clone())
        .collect();

    let mut total_rows = 0usize;
    let mut batch = Vec::with_capacity(batch_size);

    // Pre-compute target table identifier and column list for LOB blocks
    let target_ident = shentong_quote_ident(&target_table.name);
    let columns_str = target_table
        .columns
        .iter()
        .map(|col| shentong_quote_ident(&col.name))
        .collect::<Vec<_>>()
        .join(", ");

    for row_result in query_rows {
        let row = row_result.context("Error reading data row")?;

        let mut values = Vec::with_capacity(source_table.columns.len());
        let mut large_blobs: Vec<(usize, Vec<u8>)> = Vec::new();

        for (col_index, (column, logical_type)) in source_table
            .columns
            .iter()
            .zip(logical_types.iter())
            .enumerate()
        {
            let raw: Option<String> =
                row.get::<_, Option<String>>(col_index).with_context(|| {
                    format!(
                        "Failed to decode Shentong value for {}.{} column '{}'",
                        source_schema, source_table.name, column.name
                    )
                })?;
            let val = parse_value(logical_type, raw).with_context(|| {
                format!(
                    "Failed to parse Shentong value for {}.{} column '{}'",
                    source_schema, source_table.name, column.name
                )
            })?;
            // Detect large binary values that need DBMS_LOB treatment
            if let CanonicalValue::Binary(ref bytes) = val {
                if bytes.len() > SHENTONG_INLINE_BLOB_MAX_BYTES {
                    large_blobs.push((col_index, bytes.clone()));
                    values.push(CanonicalValue::Null); // placeholder
                    continue;
                }
            }
            values.push(val);
        }

        if !large_blobs.is_empty() {
            // Flush pending normal batch first
            if !batch.is_empty() {
                writeln!(
                    writer,
                    "{}",
                    renderer.render_insert_batch(target_table, &batch)?
                )?;
                total_rows += batch.len();
                batch.clear();
            }
            // Format non-blob values as SQL literals
            let formatted_values: Vec<String> = values.iter().map(shentong_format_value).collect();
            let where_predicate = shentong_primary_key_predicate(target_table, &values);
            crate::dialect::write_shentong_lob_insert_block(
                writer,
                &target_ident,
                &columns_str,
                &formatted_values,
                &large_blobs,
                where_predicate.as_deref(),
            )?;
            total_rows += 1;
        } else {
            batch.push(CanonicalRow { values });

            if batch.len() >= batch_size {
                writeln!(
                    writer,
                    "{}",
                    renderer.render_insert_batch(target_table, &batch)?
                )?;
                total_rows += batch.len();
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        writeln!(
            writer,
            "{}",
            renderer.render_insert_batch(target_table, &batch)?
        )?;
        total_rows += batch.len();
    }

    Ok(total_rows)
}

fn shentong_primary_key_predicate(
    table: &crate::domain::canonical::CanonicalTable,
    values: &[CanonicalValue],
) -> Option<String> {
    if table.primary_keys.is_empty() {
        return None;
    }

    let mut predicates = Vec::with_capacity(table.primary_keys.len());
    for pk in &table.primary_keys {
        let column_index = table
            .columns
            .iter()
            .position(|col| col.name.eq_ignore_ascii_case(pk))?;
        let value = values.get(column_index)?;
        if matches!(value, CanonicalValue::Null) {
            return None;
        }
        predicates.push(format!(
            "{} = {}",
            shentong_quote_ident(pk),
            shentong_format_value(value)
        ));
    }

    Some(predicates.join(" AND "))
}

fn parse_value(logical_type: &LogicalType, raw: Option<String>) -> Result<CanonicalValue> {
    let Some(raw) = raw else {
        return Ok(CanonicalValue::Null);
    };

    Ok(match logical_type {
        LogicalType::Integer => raw
            .trim()
            .parse::<i64>()
            .map(CanonicalValue::Integer)
            .with_context(|| format!("Invalid Shentong integer literal '{}'", raw))?,
        LogicalType::Decimal => CanonicalValue::Decimal(raw),
        LogicalType::Float => raw
            .trim()
            .parse::<f64>()
            .map(CanonicalValue::Float)
            .with_context(|| format!("Invalid Shentong float literal '{}'", raw))?,
        LogicalType::Boolean => {
            let normalized = raw.trim().to_ascii_lowercase();
            if matches!(normalized.as_str(), "1" | "true" | "t" | "y" | "yes") {
                CanonicalValue::Boolean(true)
            } else if matches!(normalized.as_str(), "0" | "false" | "f" | "n" | "no") {
                CanonicalValue::Boolean(false)
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid Shentong boolean literal '{}'",
                    raw
                ));
            }
        }
        LogicalType::Binary => parse_hex_bytes(&raw)
            .map(CanonicalValue::Binary)
            .ok_or_else(|| anyhow::anyhow!("Invalid Shentong hex binary literal '{}'", raw))?,
        LogicalType::Date => CanonicalValue::Date(raw),
        LogicalType::DateTime => CanonicalValue::DateTime(raw),
        LogicalType::Json => CanonicalValue::Json(raw),
        LogicalType::String | LogicalType::Text | LogicalType::Unknown => {
            CanonicalValue::String(raw)
        }
    })
}

fn parse_hex_bytes(raw: &str) -> Option<Vec<u8>> {
    let normalized = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if normalized.is_empty() {
        return Some(Vec::new());
    }
    if !normalized.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(normalized.len() / 2);
    for idx in (0..normalized.len()).step_by(2) {
        out.push(u8::from_str_radix(&normalized[idx..idx + 2], 16).ok()?);
    }
    Some(out)
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_value_rejects_invalid_literals() {
        assert!(parse_value(&LogicalType::Integer, Some("abc".to_string())).is_err());
        assert!(parse_value(&LogicalType::Float, Some("not-float".to_string())).is_err());
        assert!(parse_value(&LogicalType::Boolean, Some("maybe".to_string())).is_err());
        assert!(parse_value(&LogicalType::Binary, Some("ABC".to_string())).is_err());
    }

    #[test]
    fn parse_value_preserves_null_and_text() {
        assert_eq!(
            parse_value(&LogicalType::Integer, None).unwrap(),
            CanonicalValue::Null
        );
        assert_eq!(
            parse_value(&LogicalType::String, Some("hello".to_string())).unwrap(),
            CanonicalValue::String("hello".to_string())
        );
    }
}
