use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use tokio_postgres::Row;

use crate::{
    db::pg_native,
    dialect::{mysql_renderer::MySqlDialectRenderer, renderer_for, DialectRenderer},
    domain::canonical::{
        logical_type_from_db_type, CanonicalColumn, CanonicalRow, CanonicalTable, CanonicalValue,
        LogicalType,
    },
    export::{data::topological_sort_by_foreign_keys, orchestrator::LegacyExportPlan},
    models::{ConnectionConfig, DbType, ForeignKey},
};

pub async fn export_kingbase_to_mysql_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
) -> Result<()> {
    let client = pg_native::connect(config).await?;
    let renderer = MySqlDialectRenderer;

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "-- KingBase -> MySQL DDL export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    for (idx, table_id) in plan.tables.iter().enumerate() {
        let mut table = inspect_canonical_table(&client, &table_id.schema, &table_id.name)
            .await
            .with_context(|| {
                format!(
                    "Failed to inspect KingBase table '{}.{}'",
                    table_id.schema, table_id.name
                )
            })?;
        table.name = format!("{}.{}", plan.target_schema, &table_id.name);
        let ddl = renderer.render_table_ddl(&table)?;

        if idx > 0 {
            writeln!(writer)?;
        }
        writeln!(writer, "{}", ddl)?;
    }

    writer
        .flush()
        .context("Failed to flush kingbase->mysql ddl export")?;
    Ok(())
}

pub async fn export_kingbase_to_mysql_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    let client = pg_native::connect(config).await?;
    let renderer = MySqlDialectRenderer;

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut total_rows = 0usize;

    writeln!(writer, "-- KingBase -> MySQL data export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    // Phase 0: Collect FK info for all tables
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());
    for table_id in &plan.tables {
        let fks = pg_native::fetch_foreign_keys(&client, &table_id.schema, &table_id.name)
            .await
            .unwrap_or_default();
        table_names.push(table_id.name.clone());
        table_fks.push(fks);
    }

    // Compute FK-aware ordering
    let insert_order = topological_sort_by_foreign_keys(&table_names, &table_fks);
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    // Phase 1: TRUNCATE all tables (children first) — MySQL backtick syntax
    writeln!(
        writer,
        "-- Phase 1: TRUNCATE tables (children before parents)"
    )?;
    for &idx in &truncate_order {
        let table_id = &plan.tables[idx];
        writeln!(
            writer,
            "TRUNCATE TABLE `{}`.`{}`;",
            plan.target_schema.replace('`', "``"),
            table_id.name.replace('`', "``")
        )?;
    }
    writeln!(writer)?;

    // Phase 2: INSERT data (parents first)
    writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;
    for &idx in &insert_order {
        let table_id = &plan.tables[idx];
        let source_table = inspect_canonical_table(&client, &table_id.schema, &table_id.name)
            .await
            .with_context(|| {
                format!(
                    "Failed to inspect KingBase table '{}.{}'",
                    table_id.schema, table_id.name
                )
            })?;
        let mut target_table = source_table.clone();
        target_table.name = format!("{}.{}", plan.target_schema, &table_id.name);

        let count = export_table_rows(
            &client,
            &table_id.schema,
            &source_table,
            &target_table,
            &renderer,
            &mut writer,
            batch_size,
        )
        .await?;
        total_rows += count;
    }

    writer
        .flush()
        .context("Failed to flush kingbase->mysql data export")?;
    Ok(total_rows)
}

pub async fn export_kingbase_to_dm8_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
) -> Result<()> {
    let client = pg_native::connect(config).await?;
    let renderer = renderer_for(&DbType::Dm8);

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "-- KingBase -> DM8 DDL export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    for (idx, table_id) in plan.tables.iter().enumerate() {
        let mut table = inspect_canonical_table(&client, &table_id.schema, &table_id.name)
            .await
            .with_context(|| {
                format!(
                    "Failed to inspect KingBase table '{}.{}'",
                    table_id.schema, table_id.name
                )
            })?;
        table.name = format!("{}.{}", plan.target_schema, &table_id.name);
        let ddl = renderer.render_table_ddl(&table)?;

        if idx > 0 {
            writeln!(writer)?;
        }
        writeln!(writer, "{}", ddl)?;
    }

    writer
        .flush()
        .context("Failed to flush kingbase->dm8 ddl export")?;
    Ok(())
}

pub async fn export_kingbase_to_dm8_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    let client = pg_native::connect(config).await?;
    let renderer = renderer_for(&DbType::Dm8);

    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut total_rows = 0usize;

    writeln!(writer, "-- KingBase -> DM8 data export script")?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    // Phase 0: Collect FK info for all tables
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());
    for table_id in &plan.tables {
        let fks = pg_native::fetch_foreign_keys(&client, &table_id.schema, &table_id.name)
            .await
            .unwrap_or_default();
        table_names.push(table_id.name.clone());
        table_fks.push(fks);
    }

    // Compute FK-aware ordering
    let insert_order = topological_sort_by_foreign_keys(&table_names, &table_fks);
    let truncate_order: Vec<usize> = insert_order.iter().copied().rev().collect();

    // Phase 1: TRUNCATE all tables (children first) — DM8 double-quote syntax
    writeln!(
        writer,
        "-- Phase 1: TRUNCATE tables (children before parents)"
    )?;
    for &idx in &truncate_order {
        let table_id = &plan.tables[idx];
        writeln!(
            writer,
            "TRUNCATE TABLE \"{}\".\"{}\";",
            plan.target_schema.replace('"', "\"\""),
            table_id.name.replace('"', "\"\"")
        )?;
    }
    writeln!(writer)?;

    // Phase 2: INSERT data (parents first)
    writeln!(writer, "-- Phase 2: INSERT data (parents before children)")?;
    for &idx in &insert_order {
        let table_id = &plan.tables[idx];
        let source_table = inspect_canonical_table(&client, &table_id.schema, &table_id.name)
            .await
            .with_context(|| {
                format!(
                    "Failed to inspect KingBase table '{}.{}'",
                    table_id.schema, table_id.name
                )
            })?;
        let mut target_table = source_table.clone();
        target_table.name = format!("{}.{}", plan.target_schema, &table_id.name);

        let count = export_table_rows(
            &client,
            &table_id.schema,
            &source_table,
            &target_table,
            renderer.as_ref(),
            &mut writer,
            batch_size,
        )
        .await?;
        total_rows += count;
    }

    writer
        .flush()
        .context("Failed to flush kingbase->dm8 data export")?;
    Ok(total_rows)
}

async fn inspect_canonical_table(
    client: &tokio_postgres::Client,
    schema: &str,
    table: &str,
) -> Result<CanonicalTable> {
    let columns = fetch_columns(client, schema, table).await?;
    let primary_keys = fetch_primary_keys(client, schema, table).await?;

    Ok(CanonicalTable {
        name: table.trim().to_string(),
        columns,
        primary_keys,
    })
}

async fn fetch_columns(
    client: &tokio_postgres::Client,
    schema: &str,
    table: &str,
) -> Result<Vec<CanonicalColumn>> {
    let rows = client
        .query(
            "SELECT a.attname, \
                    pg_catalog.format_type(a.atttypid, a.atttypmod), \
                    a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON a.attrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[&schema, &table],
        )
        .await
        .context("Failed to query KingBase columns")?;

    let mut columns = Vec::new();
    for row in rows {
        let name: String = row.get(0);
        let data_type: String = row.get(1);
        let not_null: bool = row.get(2);

        let logical_type = logical_type_from_db_type(&data_type);

        columns.push(CanonicalColumn {
            name,
            logical_type,
            nullable: !not_null,
            identity: false,
        });
    }
    Ok(columns)
}

async fn fetch_primary_keys(
    client: &tokio_postgres::Client,
    schema: &str,
    table: &str,
) -> Result<Vec<String>> {
    let rows = client
        .query(
            "SELECT a.attname \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute a \
               ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) \
             WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'p' \
             ORDER BY array_position(con.conkey, a.attnum)",
            &[&schema, &table],
        )
        .await
        .context("Failed to query primary keys")?;

    Ok(rows.into_iter().map(|r| r.get(0)).collect())
}

async fn export_table_rows(
    client: &tokio_postgres::Client,
    source_schema: &str,
    source_table: &CanonicalTable,
    target_table: &CanonicalTable,
    renderer: &dyn DialectRenderer,
    writer: &mut BufWriter<File>,
    batch_size: usize,
) -> Result<usize> {
    if source_table.columns.is_empty() {
        return Ok(0);
    }

    let selected_columns = source_table
        .columns
        .iter()
        .map(|col| format!("\"{}\"", col.name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM \"{}\".\"{}\"",
        selected_columns,
        source_schema.replace('"', "\"\""),
        source_table.name.replace('"', "\"\"")
    );

    let row_stream = client.query_raw(&sql, std::iter::empty::<i32>()).await?;
    let logical_types = source_table
        .columns
        .iter()
        .map(|col| col.logical_type.clone())
        .collect::<Vec<_>>();

    let mut total_rows = 0usize;
    let mut batch_rows = Vec::with_capacity(batch_size);

    futures_util::pin_mut!(row_stream);
    use futures_util::stream::TryStreamExt;
    while let Some(row) = row_stream.try_next().await? {
        let mut values = Vec::with_capacity(source_table.columns.len());
        for (col_index, (column, logical_type)) in source_table
            .columns
            .iter()
            .zip(logical_types.iter())
            .enumerate()
        {
            let value = parse_row_value(&row, col_index, logical_type).with_context(|| {
                format!(
                    "Failed to decode Kingbase value for {}.{} column '{}'",
                    source_schema, source_table.name, column.name
                )
            })?;
            values.push(value);
        }
        batch_rows.push(CanonicalRow { values });

        if batch_rows.len() >= batch_size {
            writeln!(
                writer,
                "{}",
                renderer.render_insert_batch(target_table, &batch_rows)?
            )?;
            total_rows += batch_rows.len();
            batch_rows.clear();
        }
    }

    if !batch_rows.is_empty() {
        writeln!(
            writer,
            "{}",
            renderer.render_insert_batch(target_table, &batch_rows)?
        )?;
        total_rows += batch_rows.len();
    }

    Ok(total_rows)
}

fn parse_row_value(
    row: &Row,
    col_index: usize,
    logical_type: &LogicalType,
) -> Result<CanonicalValue> {
    Ok(match logical_type {
        LogicalType::Integer => row
            .try_get::<_, Option<i64>>(col_index)
            .with_context(|| format!("Failed to decode column {} as integer", col_index))?
            .map(CanonicalValue::Integer)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Decimal => row
            .try_get::<_, Option<String>>(col_index)
            .with_context(|| format!("Failed to decode column {} as decimal text", col_index))?
            .map(CanonicalValue::Decimal)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Float => row
            .try_get::<_, Option<f64>>(col_index)
            .with_context(|| format!("Failed to decode column {} as float", col_index))?
            .map(CanonicalValue::Float)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Boolean => row
            .try_get::<_, Option<bool>>(col_index)
            .with_context(|| format!("Failed to decode column {} as boolean", col_index))?
            .map(CanonicalValue::Boolean)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Binary => row
            .try_get::<_, Option<Vec<u8>>>(col_index)
            .with_context(|| format!("Failed to decode column {} as binary", col_index))?
            .map(CanonicalValue::Binary)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Date | LogicalType::DateTime => row
            .try_get::<_, Option<String>>(col_index)
            .with_context(|| format!("Failed to decode column {} as date/time text", col_index))?
            .map(|s| match logical_type {
                LogicalType::Date => CanonicalValue::Date(s),
                _ => CanonicalValue::DateTime(s),
            })
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Json => row
            .try_get::<_, Option<String>>(col_index)
            .with_context(|| format!("Failed to decode column {} as json text", col_index))?
            .map(CanonicalValue::Json)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::String | LogicalType::Text | LogicalType::Unknown => row
            .try_get::<_, Option<String>>(col_index)
            .with_context(|| format!("Failed to decode column {} as text", col_index))?
            .map(CanonicalValue::String)
            .unwrap_or(CanonicalValue::Null),
    })
}
