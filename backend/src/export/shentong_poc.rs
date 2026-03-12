use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};

use crate::{
    db::shentong as db_shentong,
    dialect::renderer_for,
    domain::canonical::{canonical_table_from_details, CanonicalRow, CanonicalValue, LogicalType},
    export::{
        data::topological_sort_by_foreign_keys,
        orchestrator::LegacyExportPlan,
    },
    models::{Column, ConnectionConfig, DbType, ForeignKey, TableDetails},
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

    // Phase 0: Collect FK info for all tables
    let fk_conn = db_shentong::open(config)?;
    let owner = plan.source_schema.trim().to_uppercase();
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());
    for table_id in &plan.tables {
        let table_upper = table_id.name.trim().to_uppercase();
        let fks = db_shentong::fetch_foreign_keys(&fk_conn, &owner, &table_upper)
            .unwrap_or_default();
        table_names.push(table_id.name.clone());
        table_fks.push(fks);
    }

    // Compute FK-aware ordering
    let insert_order = topological_sort_by_foreign_keys(&table_names, &table_fks);
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    // Phase 1: TRUNCATE all tables (children first) — double-quote syntax
    writeln!(writer, "-- Phase 1: TRUNCATE tables (children before parents)")?;
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

    // Phase 2: INSERT data (parents first)
    writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;
    for &idx in &insert_order {
        let table_id = &plan.tables[idx];
        // Open a single connection for each table — reuse it for both
        // metadata inspection and data export.
        let table_conn = db_shentong::open(config)?;
        let details = inspect_table_details(&table_conn, &plan.source_schema, &table_id.name)
            .with_context(|| format!("Failed to inspect Shentong table '{}'", table_id))?;

        let source_table = canonical_table_from_details(&details);
        let mut target_table = canonical_table_from_details(&details);
        target_table.name = format!("{}.{}", plan.target_schema, details.name);

        // Reuse the same connection for data export
        let count = export_table_rows(
            config,
            &table_conn,
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

    writer
        .flush()
        .context("Failed to flush shentong data export")?;
    Ok(total_rows)
}

fn inspect_table_details(
    conn: &shentong::Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let owner = schema.trim().to_uppercase();
    let table_name = table.trim().to_uppercase();

    let columns = fetch_columns(conn, &owner, &table_name)?;
    let primary_keys = fetch_primary_keys(conn, &owner, &table_name)?;

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

fn fetch_columns(conn: &shentong::Connection, owner: &str, table: &str) -> Result<Vec<Column>> {
    let sql = "SELECT column_name, data_type, data_length, data_precision, data_scale, \
                      nullable, data_default \
               FROM all_tab_columns \
               WHERE owner = :1 AND table_name = :2 \
               ORDER BY column_id";

    let rows = conn
        .query(sql, &[&owner, &table])
        .context("Failed to query Shentong columns")?;

    let mut columns = Vec::new();
    for row_result in rows {
        let row = row_result.context("Error reading column row")?;

        let name: String = row.get(0)?;
        let data_type: String = row.get(1)?;
        let length: Option<i32> = row.get::<_, Option<i32>>(2).unwrap_or(None);
        let precision: Option<i32> = row.get::<_, Option<i32>>(3).unwrap_or(None);
        let scale: Option<i32> = row.get::<_, Option<i32>>(4).unwrap_or(None);
        let nullable_str: String = row.get::<_, String>(5).unwrap_or_else(|_| "Y".to_string());
        let nullable = nullable_str.trim() != "N";
        let default_value: Option<String> = row.get::<_, Option<String>>(6).unwrap_or(None);

        columns.push(Column {
            name,
            data_type,
            length,
            precision,
            scale,
            char_semantics: None,
            nullable,
            comment: None,
            default_value,
            identity: false,
            identity_start: None,
            identity_increment: None,
        });
    }
    Ok(columns)
}

fn fetch_primary_keys(conn: &shentong::Connection, owner: &str, table: &str) -> Result<Vec<String>> {
    let sql = "SELECT acc.column_name \
               FROM all_constraints ac \
               JOIN all_cons_columns acc \
                 ON ac.constraint_name = acc.constraint_name \
                AND ac.owner = acc.owner \
               WHERE ac.constraint_type = 'P' \
                 AND ac.owner = :1 \
                 AND ac.table_name = :2 \
               ORDER BY acc.position";

    let rows = conn
        .query(sql, &[&owner, &table])
        .context("Failed to query Shentong primary keys")?;

    let mut keys = Vec::new();
    for row_result in rows {
        let row = row_result.context("Error reading pk row")?;
        keys.push(row.get::<_, String>(0)?);
    }
    Ok(keys)
}

fn export_table_rows(
    _config: &ConnectionConfig,
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
    let set_sql = format!("SET search_path TO {}, public", source_schema);
    tracing::debug!(sql = %set_sql, "Setting Shentong search_path");
    match conn.execute(&set_sql, &[]) {
        Ok(_) => tracing::debug!("search_path set successfully"),
        Err(e) => tracing::warn!(error = ?e, "Failed to set search_path"),
    }

    // Now query using unqualified table name
    let selected_columns_unquoted = source_table
        .columns
        .iter()
        .map(|col| col.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!("SELECT {} FROM {}", selected_columns_unquoted, source_table.name);

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

    for row_result in query_rows {
        let row = row_result.context("Error reading data row")?;

        let mut values = Vec::with_capacity(source_table.columns.len());
        for (col_index, logical_type) in logical_types.iter().enumerate() {
            let raw: Option<String> = row.get::<_, Option<String>>(col_index).unwrap_or(None);
            values.push(parse_value(logical_type, raw));
        }
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

fn parse_value(logical_type: &LogicalType, raw: Option<String>) -> CanonicalValue {
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
        LogicalType::Binary => parse_hex_bytes(&raw)
            .map(CanonicalValue::Binary)
            .unwrap_or_else(|| CanonicalValue::Binary(raw.into_bytes())),
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
        out.push(u8::from_str_radix(&normalized[idx..idx + 2], 16).ok()?);
    }
    Some(out)
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
