use std::collections::HashMap;

use anyhow::{anyhow, ensure, Context, Result};
use odbc_api::{Connection, Cursor, buffers::TextRowSet};

use crate::models::{Column, Index, Table, TableDetails};

pub fn get_tables(connection: &Connection<'_>, schema: &str) -> Result<Vec<Table>> {
    let owner = schema.to_uppercase();

    let sql = format!(
        "SELECT t.TABLE_NAME, c.COMMENTS, NVL(t.NUM_ROWS, 0) AS NUM_ROWS \
         FROM ALL_TABLES t \
         LEFT JOIN ALL_TAB_COMMENTS c ON t.OWNER = c.OWNER AND t.TABLE_NAME = c.TABLE_NAME \
         WHERE t.OWNER = '{}' \
         ORDER BY t.TABLE_NAME",
        owner.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query DM8 tables")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for tables query"))?;

    let batch_size = 100;
    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut tables = Vec::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Encountered table without a name in DM8 metadata"))?
                .to_string();
            let comment = batch.at_as_str(1, row_index)?.map(|s| s.to_string());
            let row_count = batch.at_as_str(2, row_index)?
                .and_then(|s| s.parse::<i64>().ok());

            tables.push(Table {
                name,
                comment,
                row_count,
            });
        }
    }

    // Fallback: if NUM_ROWS is缺失或为 0，则实时 COUNT(*)
    for table in &mut tables {
        if table.row_count.is_none() || table.row_count == Some(0) {
            table.row_count = fetch_row_count(connection, &owner, &table.name).ok();
        }
    }

    Ok(tables)
}

pub fn get_table_details(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let owner = schema.to_uppercase();
    let table_name = table.to_uppercase();

    let comment = fetch_table_comment(connection, &owner, &table_name)?;

    let columns = fetch_columns(connection, &owner, &table_name)
        .with_context(|| format!("Failed to fetch columns for table {}", table_name))?;
    ensure!(
        !columns.is_empty(),
        "Table '{}' does not exist in schema '{}'",
        table_name,
        owner
    );

    let primary_keys = fetch_primary_keys(connection, &owner, &table_name)?;
    let indexes = fetch_indexes(connection, &owner, &table_name)?;

    Ok(TableDetails {
        name: table_name,
        comment,
        columns,
        primary_keys,
        indexes,
    })
}

fn fetch_table_comment(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Option<String>> {
    let sql = format!(
        "SELECT COMMENTS FROM ALL_TAB_COMMENTS WHERE OWNER = '{}' AND TABLE_NAME = '{}'",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = match connection
        .execute(&sql, ())
        .context("Failed to query table comment")?
    {
        Some(cursor) => cursor,
        None => return Ok(None),
    };

    let mut buffers = TextRowSet::for_cursor(1, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            let comment = batch.at_as_str(0, 0)?.map(|s| s.to_string());
            return Ok(comment);
        }
    }

    Ok(None)
}

fn fetch_columns(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<Column>> {
    let sql = format!(
        "SELECT c.COLUMN_NAME, c.DATA_TYPE, c.DATA_LENGTH, c.NULLABLE, cc.COMMENTS \
         FROM ALL_TAB_COLUMNS c \
         LEFT JOIN ALL_COL_COMMENTS cc ON cc.OWNER = c.OWNER AND cc.TABLE_NAME = c.TABLE_NAME AND cc.COLUMN_NAME = c.COLUMN_NAME \
         WHERE c.OWNER = '{}' AND c.TABLE_NAME = '{}' \
         ORDER BY c.COLUMN_ID",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query DM8 columns")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for column query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut columns = Vec::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Encountered column without a name"))?
                .to_string();
            let data_type = batch.at_as_str(1, row_index)?
                .ok_or_else(|| anyhow!("Encountered column without data type"))?
                .to_string();
            let length = batch.at_as_str(2, row_index)?
                .and_then(|s| s.parse::<i32>().ok());
            let nullable_flag = batch.at_as_str(3, row_index)?;
            let nullable = matches!(nullable_flag, Some(flag) if flag.eq_ignore_ascii_case("Y"));
            let comment = batch.at_as_str(4, row_index)?.map(|s| s.to_string());

            columns.push(Column {
                name,
                data_type,
                length,
                nullable,
                comment,
            });
        }
    }

    Ok(columns)
}

fn fetch_row_count(connection: &Connection<'_>, schema: &str, table: &str) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) AS CNT FROM \"{}\".\"{}\"",
        schema.replace('"', "\"\""),
        table.replace('"', "\"\"")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .with_context(|| format!("Failed to count rows for table {}", table))?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for row count query"))?;

    let mut buffers = TextRowSet::for_cursor(1, &mut cursor, Some(32))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            if let Some(val) = batch.at_as_str(0, 0)? {
                if let Ok(count) = val.parse::<i64>() {
                    return Ok(count);
                }
            }
        }
    }

    Err(anyhow!("Failed to read row count for {}", table))
}

fn fetch_primary_keys(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT acc.COLUMN_NAME \
         FROM ALL_CONSTRAINTS ac \
         JOIN ALL_CONS_COLUMNS acc ON ac.OWNER = acc.OWNER AND ac.CONSTRAINT_NAME = acc.CONSTRAINT_NAME \
         WHERE ac.CONSTRAINT_TYPE = 'P' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY acc.POSITION",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query primary keys")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for primary key query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut keys = Vec::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Primary key column name missing"))?
                .to_string();
            keys.push(name);
        }
    }

    Ok(keys)
}

fn fetch_indexes(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<Index>> {
    let sql = format!(
        "SELECT ai.INDEX_NAME, ai.UNIQUENESS \
         FROM ALL_INDEXES ai \
         WHERE ai.TABLE_OWNER = '{}' AND ai.TABLE_NAME = '{}' \
         ORDER BY ai.INDEX_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query indexes")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for index query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut order = Vec::new();
    let mut indexes = HashMap::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Index name missing"))?
                .to_string();
            let uniqueness = batch.at_as_str(1, row_index)?;
            let unique = matches!(
                uniqueness,
                Some(flag) if flag.eq_ignore_ascii_case("UNIQUE") || flag.eq_ignore_ascii_case("Y")
            );

            order.push(name.clone());
            indexes.insert(
                name.clone(),
                Index {
                    name,
                    columns: Vec::new(),
                    unique,
                },
            );
        }
    }

    // Fetch index columns
    let sql = format!(
        "SELECT ic.INDEX_NAME, ic.COLUMN_NAME \
         FROM ALL_IND_COLUMNS ic \
         WHERE ic.INDEX_OWNER = '{}' AND ic.TABLE_NAME = '{}' \
         ORDER BY ic.INDEX_NAME, ic.COLUMN_POSITION",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut column_cursor = match connection
        .execute(&sql, ())
        .context("Failed to query index columns")?
    {
        Some(cursor) => cursor,
        None => return Ok(order.into_iter().filter_map(|name| indexes.remove(&name)).collect()),
    };

    let mut col_buffers = TextRowSet::for_cursor(100, &mut column_cursor, Some(8192))?;
    let mut col_row_set_cursor = column_cursor.bind_buffer(&mut col_buffers)?;

    while let Some(batch) = col_row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let index_name = match batch.at_as_str(0, row_index)? {
                Some(val) => val,
                None => continue,
            };
            let column_name = match batch.at_as_str(1, row_index)? {
                Some(val) => val.to_string(),
                None => continue,
            };

            if let Some(index) = indexes.get_mut(index_name) {
                index.columns.push(column_name);
            }
        }
    }

    let mut result = Vec::new();
    for name in order {
        if let Some(index) = indexes.remove(&name) {
            result.push(index);
        }
    }

    Ok(result)
}
