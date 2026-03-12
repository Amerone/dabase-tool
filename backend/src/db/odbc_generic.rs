use anyhow::{anyhow, Context, Result};
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::db::pool::ConnectionPool;
use crate::models::{
    CheckConstraint, Column, ConnectionConfig, ForeignKey, Index, Table, TableDetails,
    TriggerDefinition, UniqueConstraint,
};

fn decode_cell(batch: &TextRowSet, col: usize, row: usize) -> Option<String> {
    match batch.at_as_str(col, row) {
        Ok(Some(s)) => Some(s.to_string()),
        Ok(None) => None,
        Err(_) => batch
            .at(col, row)
            .map(|bytes| encoding_rs::GB18030.decode(bytes).0.into_owned()),
    }
}

pub fn test_connection(config: &ConnectionConfig) -> Result<()> {
    config
        .validate()
        .context("Invalid ODBC connection configuration")?;
    let pool = ConnectionPool::new(config.clone())?;
    pool.test_connection()
}

pub fn get_schemas(config: &ConnectionConfig) -> Result<Vec<String>> {
    let pool = ConnectionPool::new_for_schema_discovery(config.clone())?;
    let connection = pool
        .get_connection()
        .context("Failed to open ODBC connection")?;

    let sql = "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name";

    let mut cursor = connection
        .execute(sql, ())
        .context("Failed to query schemas via information_schema")?
        .ok_or_else(|| anyhow!("ODBC returned no cursor for schema query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut schemas = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            if let Some(name) = decode_cell(batch, 0, row_index) {
                schemas.push(name);
            }
        }
    }

    Ok(schemas)
}

pub fn get_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    config
        .validate()
        .context("Invalid ODBC connection configuration")?;

    let pool = ConnectionPool::new(config.clone())?;
    let connection = pool
        .get_connection()
        .context("Failed to open ODBC connection")?;

    let schema = config.schema.trim().replace('"', "\"\"");
    let sql = format!(
        "SELECT table_schema, table_name, NULL AS table_comment, NULL AS row_count \
         FROM information_schema.tables \
         WHERE table_schema = '{}' AND table_type = 'BASE TABLE' \
         ORDER BY table_name",
        schema.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query tables via information_schema")?
        .ok_or_else(|| anyhow!("ODBC returned no cursor for tables query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut tables = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let schema = decode_cell(batch, 0, row_index);
            let name = decode_cell(batch, 1, row_index)
                .ok_or_else(|| anyhow!("Encountered table without a name"))?;
            tables.push(Table {
                schema,
                name,
                comment: None,
                row_count: None,
            });
        }
    }

    Ok(tables)
}

pub fn get_table_details(config: &ConnectionConfig, schema: &str, table: &str) -> Result<TableDetails> {
    config
        .validate()
        .context("Invalid ODBC connection configuration")?;

    let pool = ConnectionPool::new(config.clone())?;
    let connection = pool
        .get_connection()
        .context("Failed to open ODBC connection")?;

    let schema_escaped = schema.trim().replace('\'', "''");
    let table_name = table.trim().replace('\'', "''");

    let columns = fetch_columns(&connection, &schema_escaped, &table_name)?;
    let primary_keys = fetch_primary_keys(&connection, &schema_escaped, &table_name)?;

    Ok(TableDetails {
        name: table.trim().to_string(),
        comment: None,
        columns,
        primary_keys,
        indexes: Vec::<Index>::new(),
        unique_constraints: Vec::<UniqueConstraint>::new(),
        foreign_keys: Vec::<ForeignKey>::new(),
        check_constraints: Vec::<CheckConstraint>::new(),
        triggers: Vec::<TriggerDefinition>::new(),
    })
}

fn fetch_columns(connection: &Connection<'_>, schema: &str, table: &str) -> Result<Vec<Column>> {
    let sql = format!(
        "SELECT column_name, data_type, character_maximum_length, numeric_precision, numeric_scale, \
                is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = '{}' AND table_name = '{}' \
         ORDER BY ordinal_position",
        schema, table
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query columns via information_schema")?
        .ok_or_else(|| anyhow!("ODBC returned no cursor for column query"))?;

    let mut buffers = TextRowSet::for_cursor(400, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut columns = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let nullable = decode_cell(batch, 5, row_index)
                .map(|v| v.eq_ignore_ascii_case("YES"))
                .unwrap_or(true);

            columns.push(Column {
                name: decode_cell(batch, 0, row_index)
                    .ok_or_else(|| anyhow!("Column name missing"))?,
                data_type: decode_cell(batch, 1, row_index)
                    .ok_or_else(|| anyhow!("Column data type missing"))?,
                length: decode_cell(batch, 2, row_index).and_then(|v| v.parse::<i32>().ok()),
                precision: decode_cell(batch, 3, row_index).and_then(|v| v.parse::<i32>().ok()),
                scale: decode_cell(batch, 4, row_index).and_then(|v| v.parse::<i32>().ok()),
                char_semantics: None,
                nullable,
                comment: None,
                default_value: decode_cell(batch, 6, row_index),
                identity: false,
                identity_start: None,
                identity_increment: None,
            });
        }
    }

    Ok(columns)
}

fn fetch_primary_keys(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
          AND tc.table_name = kcu.table_name \
         WHERE tc.constraint_type = 'PRIMARY KEY' \
           AND tc.table_schema = '{}' \
           AND tc.table_name = '{}' \
         ORDER BY kcu.ordinal_position",
        schema, table
    );

    let mut cursor = match connection
        .execute(&sql, ())
        .context("Failed to query primary keys via information_schema")?
    {
        Some(cursor) => cursor,
        None => return Ok(Vec::new()),
    };

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(2048))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut keys = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            if let Some(name) = decode_cell(batch, 0, row_index) {
                keys.push(name);
            }
        }
    }

    Ok(keys)
}
