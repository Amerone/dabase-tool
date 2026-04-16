use anyhow::{Context, Result};
use shentong::Connection;

use crate::models::{Column, ConnectionConfig, DbType, ForeignKey, Table, TableDetails};

/// Build a ShenTong ACI connect string: `host:port` or `host:port/database`
/// When `config.database` is set (non-empty), generates `host:port/database`
/// to target a specific instance (e.g. `OSRDB`). Otherwise plain `host:port`.
fn easy_connect(config: &ConnectionConfig) -> String {
    let base = format!("{}:{}", config.host.trim(), config.port);
    match config
        .database
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(db) => format!("{}/{}", base, db),
        None => base,
    }
}

pub fn open(config: &ConnectionConfig) -> Result<Connection> {
    debug_assert_eq!(config.db_type, DbType::Shentong);
    let connect_string = easy_connect(config);
    Connection::connect(config.username.trim(), &config.password, &connect_string).with_context(
        || {
            format!(
                "Failed to connect to ShenTong at {}:{} as {}",
                config.host, config.port, config.username
            )
        },
    )
}

pub fn test_connection(config: &ConnectionConfig) -> Result<()> {
    let conn = open(config)?;
    let _rows = conn
        .query("SELECT 1 FROM DUAL", &[])
        .context("Connected to ShenTong but health query failed")?;
    Ok(())
}

/// Return all user/schema names in the OSCAR instance.
pub fn get_schemas(config: &ConnectionConfig) -> Result<Vec<String>> {
    let conn = open(config)?;
    let rows = conn
        .query_as::<String>("SELECT username FROM all_users ORDER BY username", &[])
        .context("Failed to query OSCAR user schemas")?;

    let mut schemas = Vec::new();
    for (idx, row_result) in rows.enumerate() {
        schemas.push(row_result.with_context(|| format!("Error reading schema row {}", idx))?);
    }
    Ok(schemas)
}

/// List base tables owned by the schema specified in `config.schema`.
/// In OSCAR, schema == owner == username namespace.
pub fn get_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    let conn = open(config)?;
    let owner = config.schema.trim().to_uppercase();

    let rows = conn
        .query(
            "SELECT owner, table_name FROM all_tables \
             WHERE owner = :1 ORDER BY table_name",
            &[&owner],
        )
        .context("Failed to query OSCAR tables")?;

    let mut tables = Vec::new();
    for row_result in rows {
        let row = row_result.context("Error reading table row")?;
        let schema: String = row.get(0)?;
        let name: String = row.get(1)?;
        tables.push(Table {
            schema: Some(schema),
            name,
            comment: None,
            row_count: None,
        });
    }
    Ok(tables)
}

pub fn get_table_details(
    config: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let conn = open(config)?;
    let owner = schema.trim().to_uppercase();
    let table_name = table.trim().to_uppercase();

    let columns = fetch_columns(&conn, &owner, &table_name)?;
    let primary_keys = fetch_primary_keys(&conn, &owner, &table_name)?;

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

pub fn fetch_columns(conn: &Connection, owner: &str, table: &str) -> Result<Vec<Column>> {
    let sql = "SELECT column_name, data_type, data_length, data_precision, data_scale, \
                      nullable, data_default \
               FROM all_tab_columns \
               WHERE owner = :1 AND table_name = :2 \
               ORDER BY column_id";

    let rows = conn
        .query(sql, &[&owner, &table])
        .context("Failed to query OSCAR columns")?;

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

pub fn fetch_primary_keys(conn: &Connection, owner: &str, table: &str) -> Result<Vec<String>> {
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
        .context("Failed to query OSCAR primary keys")?;

    let mut keys = Vec::new();
    for row_result in rows {
        let row = row_result.context("Error reading pk row")?;
        keys.push(row.get::<_, String>(0)?);
    }
    Ok(keys)
}

/// Fetch foreign key constraints for a table using Oracle-compatible
/// ALL_CONSTRAINTS + ALL_CONS_COLUMNS system views.
pub fn fetch_foreign_keys(conn: &Connection, owner: &str, table: &str) -> Result<Vec<ForeignKey>> {
    let sql = "SELECT ac.constraint_name, \
                      acc.column_name, \
                      ac_ref.table_name AS referenced_table, \
                      acc_ref.column_name AS referenced_column, \
                      ac.delete_rule, \
                      acc.position \
               FROM all_constraints ac \
               JOIN all_cons_columns acc \
                 ON ac.constraint_name = acc.constraint_name \
                AND ac.owner = acc.owner \
               JOIN all_constraints ac_ref \
                 ON ac.r_constraint_name = ac_ref.constraint_name \
                AND ac.r_owner = ac_ref.owner \
               JOIN all_cons_columns acc_ref \
                 ON ac_ref.constraint_name = acc_ref.constraint_name \
                AND ac_ref.owner = acc_ref.owner \
                AND acc.position = acc_ref.position \
               WHERE ac.constraint_type = 'R' \
                 AND ac.owner = :1 \
                 AND ac.table_name = :2 \
               ORDER BY ac.constraint_name, acc.position";

    let rows = conn
        .query(sql, &[&owner, &table])
        .context("Failed to query OSCAR foreign keys")?;

    use std::collections::HashMap;

    #[derive(Default)]
    struct FkAgg {
        referenced_table: String,
        delete_rule: Option<String>,
        columns: Vec<(i32, String)>,
        referenced_columns: Vec<(i32, String)>,
    }

    let mut grouped: HashMap<String, FkAgg> = HashMap::new();
    for row_result in rows {
        let row = row_result.context("Error reading FK row")?;
        let name: String = row.get(0)?;
        let column: String = row.get(1)?;
        let ref_table: String = row.get(2)?;
        let ref_column: String = row.get(3)?;
        let delete_rule: Option<String> = row.get::<_, Option<String>>(4).unwrap_or(None);
        let position: i32 = row.get::<_, i32>(5).unwrap_or(1);

        let agg = grouped.entry(name).or_default();
        if agg.referenced_table.is_empty() {
            agg.referenced_table = ref_table;
        }
        if agg.delete_rule.is_none() {
            agg.delete_rule = delete_rule;
        }
        agg.columns.push((position, column));
        agg.referenced_columns.push((position, ref_column));
    }

    let mut fks: Vec<ForeignKey> = grouped
        .into_iter()
        .map(|(name, mut agg)| {
            agg.columns.sort_by_key(|(pos, _)| *pos);
            agg.referenced_columns.sort_by_key(|(pos, _)| *pos);
            ForeignKey {
                name,
                columns: agg.columns.into_iter().map(|(_, c)| c).collect(),
                referenced_table: format!("{}.{}", owner, agg.referenced_table),
                referenced_columns: agg.referenced_columns.into_iter().map(|(_, c)| c).collect(),
                delete_rule: agg.delete_rule,
                update_rule: None,
            }
        })
        .collect();
    fks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fks)
}
