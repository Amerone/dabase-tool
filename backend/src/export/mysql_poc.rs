use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use futures_util::TryStreamExt;
use sqlx::{MySqlConnection, Row};

use crate::{
    db::mysql,
    dialect::{renderer_for, DialectRenderer},
    domain::canonical::{
        logical_type_from_db_type, CanonicalColumn, CanonicalRow, CanonicalTable, CanonicalValue,
        LogicalType,
    },
    export::{data::topological_sort_by_foreign_keys, orchestrator::LegacyExportPlan},
    models::{ConnectionConfig, DbType, ForeignKey},
};

pub async fn export_mysql_to_mysql_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
) -> Result<()> {
    export_mysql_to_target_ddl(config, plan, DbType::Mysql).await
}

pub async fn export_mysql_to_target_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    target_dialect: DbType,
) -> Result<()> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let renderer = renderer_for(&target_dialect);
    let pool = mysql::create_pool(config, 2).await?;
    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire MySQL connection")?;

    writeln!(writer, "-- MySQL -> {:?} DDL export script", target_dialect)?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    for (idx, table_id) in plan.tables.iter().enumerate() {
        let mut table = inspect_canonical_table(&mut conn, &plan.source_schema, &table_id.name)
            .await
            .with_context(|| format!("Failed to inspect table '{}'", table_id))?;
        table.name = format!("{}.{}", plan.target_schema, &table_id.name);
        let ddl = renderer.render_table_ddl(&table)?;
        if idx > 0 {
            writeln!(writer)?;
        }
        writeln!(writer, "{}", ddl)?;
    }

    writer.flush().context("Failed to flush mysql ddl export")?;
    pool.close().await;
    Ok(())
}

pub async fn export_mysql_to_mysql_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    export_mysql_to_target_data(config, plan, DbType::Mysql, batch_size).await
}

pub async fn export_mysql_to_target_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    target_dialect: DbType,
    batch_size: usize,
) -> Result<usize> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let renderer = renderer_for(&target_dialect);
    let pool = mysql::create_pool(config, 2).await?;
    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire MySQL connection")?;
    let effective_batch_size = batch_size.max(1);

    writeln!(
        writer,
        "-- MySQL -> {:?} data export script",
        target_dialect
    )?;
    writeln!(writer, "-- source schema: {}", plan.source_schema)?;
    writeln!(writer, "-- target schema: {}", plan.target_schema)?;
    writeln!(writer)?;

    // Keep one consistent snapshot for the full export so no-PK tables can be exported online
    // without duplicate/missing rows caused by concurrent writes.
    begin_read_only_snapshot(&mut conn).await?;

    let result = export_tables_data(
        &mut conn,
        plan,
        renderer.as_ref(),
        effective_batch_size,
        &mut writer,
    )
    .await;

    if result.is_ok() {
        sqlx::query("COMMIT")
            .execute(&mut *conn)
            .await
            .context("Failed to commit MySQL snapshot transaction")?;
    } else {
        let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
    }

    let total_rows = result?;

    pool.close().await;
    writer
        .flush()
        .context("Failed to flush mysql data export")?;
    Ok(total_rows)
}

async fn export_tables_data(
    conn: &mut MySqlConnection,
    plan: &LegacyExportPlan,
    renderer: &dyn DialectRenderer,
    batch_size: usize,
    writer: &mut impl Write,
) -> Result<usize> {
    let mut total_rows = 0usize;

    // Phase 0: Collect FK info for all tables
    // We need a pool for fetch_foreign_keys; create a temporary one-connection pool
    // by re-using the existing connection's transaction context via direct queries.
    let mut table_names = Vec::with_capacity(plan.tables.len());
    let mut table_fks: Vec<Vec<ForeignKey>> = Vec::with_capacity(plan.tables.len());
    for table_id in &plan.tables {
        let fks = fetch_fks_inline(conn, &plan.source_schema, &table_id.name).await?;
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
        let source_table = inspect_canonical_table(conn, &plan.source_schema, &table_id.name)
            .await
            .with_context(|| format!("Failed to inspect table '{}'", table_id))?;
        let mut target_table = source_table.clone();
        target_table.name = format!("{}.{}", plan.target_schema, &table_id.name);

        let row_count = export_table_rows_stream(
            conn,
            &plan.source_schema,
            &source_table,
            &target_table,
            renderer,
            batch_size,
            writer,
        )
        .await
        .with_context(|| format!("Failed to stream data for '{}'", table_id))?;

        if row_count > 0 {
            writeln!(writer)?;
        }
        total_rows += row_count;
    }

    Ok(total_rows)
}

/// Fetch foreign keys for a single table using the existing MySQL connection
/// (works within an active transaction, unlike pool-based fetch_foreign_keys).
async fn fetch_fks_inline(
    conn: &mut MySqlConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>> {
    use std::collections::HashMap;

    let rows = sqlx::query(
        "SELECT rc.CONSTRAINT_NAME, \
                kcu.COLUMN_NAME, \
                kcu.REFERENCED_TABLE_NAME, \
                kcu.REFERENCED_COLUMN_NAME, \
                kcu.ORDINAL_POSITION \
         FROM information_schema.REFERENTIAL_CONSTRAINTS rc \
         JOIN information_schema.KEY_COLUMN_USAGE kcu \
           ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
          AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
         WHERE rc.CONSTRAINT_SCHEMA = ? \
           AND kcu.TABLE_NAME = ? \
         ORDER BY rc.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .context("Failed to query MySQL foreign keys")?;

    #[derive(Default)]
    struct FkAgg {
        referenced_table: Option<String>,
        columns: Vec<(u64, String)>,
        referenced_columns: Vec<(u64, String)>,
    }

    let mut grouped: HashMap<String, FkAgg> = HashMap::new();
    for row in rows {
        let name: String = row.get("CONSTRAINT_NAME");
        let seq: u64 = row.try_get::<u64, _>("ORDINAL_POSITION").unwrap_or(1);
        let column: String = row.get("COLUMN_NAME");
        let referenced_table: String = row.get("REFERENCED_TABLE_NAME");
        let referenced_column: String = row.get("REFERENCED_COLUMN_NAME");

        let agg = grouped.entry(name).or_default();
        if agg.referenced_table.is_none() {
            agg.referenced_table = Some(referenced_table);
        }
        agg.columns.push((seq, column));
        agg.referenced_columns.push((seq, referenced_column));
    }

    let mut fks: Vec<ForeignKey> = grouped
        .into_iter()
        .map(|(name, mut agg)| {
            agg.columns.sort_by_key(|(pos, _)| *pos);
            agg.referenced_columns.sort_by_key(|(pos, _)| *pos);
            ForeignKey {
                name,
                columns: agg.columns.into_iter().map(|(_, c)| c).collect(),
                referenced_table: format!(
                    "{}.{}",
                    schema,
                    agg.referenced_table.unwrap_or_default()
                ),
                referenced_columns: agg.referenced_columns.into_iter().map(|(_, c)| c).collect(),
                delete_rule: None,
                update_rule: None,
            }
        })
        .collect();
    fks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fks)
}

async fn begin_read_only_snapshot(conn: &mut MySqlConnection) -> Result<()> {
    sqlx::query("SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *conn)
        .await
        .context("Failed to set MySQL transaction isolation level")?;
    sqlx::query("START TRANSACTION READ ONLY WITH CONSISTENT SNAPSHOT")
        .execute(&mut *conn)
        .await
        .context("Failed to start MySQL consistent snapshot transaction")?;
    Ok(())
}

async fn inspect_canonical_table(
    conn: &mut MySqlConnection,
    schema: &str,
    table_name: &str,
) -> Result<CanonicalTable> {
    let schema_name = schema.trim();
    let table_name = table_name.trim();

    let column_rows = sqlx::query(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
         ORDER BY ORDINAL_POSITION",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(&mut *conn)
    .await
    .context("Failed to load MySQL column metadata")?;

    anyhow::ensure!(
        !column_rows.is_empty(),
        "Table '{}' does not exist in schema '{}'",
        table_name,
        schema_name
    );

    let columns = column_rows
        .into_iter()
        .map(|row| {
            let name = row.get::<String, _>("COLUMN_NAME");
            let data_type = row.get::<String, _>("COLUMN_TYPE");
            let nullable = row
                .try_get::<String, _>("IS_NULLABLE")
                .map(|v| v.eq_ignore_ascii_case("YES"))
                .unwrap_or(true);

            CanonicalColumn {
                name,
                logical_type: logical_type_from_db_type(&data_type),
                nullable,
                identity: false,
            }
        })
        .collect::<Vec<_>>();

    let pk_rows = sqlx::query(
        "SELECT kcu.COLUMN_NAME \
         FROM information_schema.TABLE_CONSTRAINTS tc \
         JOIN information_schema.KEY_COLUMN_USAGE kcu \
           ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
          AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
          AND tc.TABLE_NAME = kcu.TABLE_NAME \
         WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' \
           AND tc.TABLE_SCHEMA = ? \
           AND tc.TABLE_NAME = ? \
         ORDER BY kcu.ORDINAL_POSITION",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(&mut *conn)
    .await
    .context("Failed to load MySQL primary-key metadata")?;

    let primary_keys = pk_rows
        .into_iter()
        .map(|row| row.get::<String, _>("COLUMN_NAME"))
        .collect::<Vec<_>>();

    Ok(CanonicalTable {
        name: table_name.to_string(),
        columns,
        primary_keys,
    })
}

async fn export_table_rows_stream(
    conn: &mut MySqlConnection,
    schema: &str,
    source_table: &CanonicalTable,
    target_table: &CanonicalTable,
    renderer: &dyn DialectRenderer,
    batch_size: usize,
    writer: &mut impl Write,
) -> Result<usize> {
    ensure_identifier_not_empty(schema, "schema")?;
    ensure_identifier_not_empty(&source_table.name, "table")?;
    for col in &source_table.columns {
        ensure_identifier_not_empty(&col.name, "column")?;
    }

    let select_cols = source_table
        .columns
        .iter()
        .map(select_expr)
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM {}.{}",
        select_cols,
        quote_ident(schema),
        quote_ident(&source_table.name)
    );

    let mut stream = sqlx::query(&sql).fetch(&mut *conn);
    let mut batch_rows = Vec::with_capacity(batch_size);
    let mut total_rows = 0usize;

    while let Some(row) = stream.try_next().await? {
        batch_rows.push(row_to_canonical(row, source_table)?);
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

fn row_to_canonical(row: sqlx::mysql::MySqlRow, table: &CanonicalTable) -> Result<CanonicalRow> {
    let mut values = Vec::with_capacity(table.columns.len());
    for col in &table.columns {
        let raw: Option<String> = row
            .try_get(col.name.as_str())
            .with_context(|| format!("Failed to decode column '{}'", col.name))?;
        values.push(parse_value(&col.logical_type, raw));
    }
    Ok(CanonicalRow { values })
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
        LogicalType::Binary => parse_hex(raw)
            .map(CanonicalValue::Binary)
            .unwrap_or(CanonicalValue::Null),
        LogicalType::Date => CanonicalValue::Date(raw),
        LogicalType::DateTime => CanonicalValue::DateTime(raw),
        LogicalType::Json => CanonicalValue::Json(raw),
        LogicalType::String | LogicalType::Text | LogicalType::Unknown => {
            CanonicalValue::String(raw)
        }
    }
}

fn parse_hex(raw: String) -> Option<Vec<u8>> {
    let normalized = raw.trim();
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

fn select_expr(col: &crate::domain::canonical::CanonicalColumn) -> String {
    let ident = quote_ident(&col.name);
    let alias = quote_ident(&col.name);
    match col.logical_type {
        LogicalType::Binary => format!("HEX({}) AS {}", ident, alias),
        _ => format!("CAST({} AS CHAR) AS {}", ident, alias),
    }
}

fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn ensure_identifier_not_empty(value: &str, kind: &str) -> Result<()> {
    let trimmed = value.trim();
    anyhow::ensure!(!trimmed.is_empty(), "{} identifier cannot be empty", kind);
    anyhow::ensure!(
        !trimmed.contains('\0'),
        "{} identifier contains NUL byte",
        kind
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_identifier_not_empty, parse_hex, parse_value, select_expr};
    use crate::domain::canonical::{CanonicalColumn, CanonicalValue, LogicalType};

    #[test]
    fn parse_hex_decodes_bytes() {
        assert_eq!(parse_hex("0AFF".to_string()), Some(vec![0x0A, 0xFF]));
        assert_eq!(parse_hex("".to_string()), Some(vec![]));
        assert_eq!(parse_hex("ABC".to_string()), None);
    }

    #[test]
    fn parse_value_handles_boolean_and_integer() {
        assert_eq!(
            parse_value(&LogicalType::Boolean, Some("true".to_string())),
            CanonicalValue::Boolean(true)
        );
        assert_eq!(
            parse_value(&LogicalType::Integer, Some("42".to_string())),
            CanonicalValue::Integer(42)
        );
        assert_eq!(
            parse_value(&LogicalType::Integer, None),
            CanonicalValue::Null
        );
    }

    #[test]
    fn select_expr_uses_hex_for_binary() {
        let binary_col = CanonicalColumn {
            name: "payload".to_string(),
            logical_type: LogicalType::Binary,
            nullable: true,
            identity: false,
        };
        let string_col = CanonicalColumn {
            name: "name".to_string(),
            logical_type: LogicalType::String,
            nullable: true,
            identity: false,
        };
        assert_eq!(select_expr(&binary_col), "HEX(`payload`) AS `payload`");
        assert_eq!(select_expr(&string_col), "CAST(`name` AS CHAR) AS `name`");
    }

    #[test]
    fn ensure_identifier_not_empty_allows_non_ascii_and_symbols() {
        assert!(ensure_identifier_not_empty("订单-明细", "table").is_ok());
        assert!(ensure_identifier_not_empty("user name", "column").is_ok());
        assert!(ensure_identifier_not_empty("a`b", "column").is_ok());
    }

    #[test]
    fn ensure_identifier_not_empty_rejects_empty_and_nul() {
        assert!(ensure_identifier_not_empty("   ", "table").is_err());
        assert!(ensure_identifier_not_empty("bad\0name", "table").is_err());
    }
}
