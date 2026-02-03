use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{anyhow, ensure, Context, Result};
use odbc_api::{Connection, Cursor, buffers::TextRowSet};

use crate::models::{
    CheckConstraint, Column, ForeignKey, Index, Sequence, Table, TableDetails, TriggerDefinition,
    UniqueConstraint,
};

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
    let unique_constraints = fetch_unique_constraints(connection, &owner, &table_name)?;
    let foreign_keys = fetch_foreign_keys(connection, &owner, &table_name)?;
    let check_constraints = fetch_check_constraints(connection, &owner, &table_name)?;
    let triggers = fetch_triggers(connection, &owner, &table_name)?;

    Ok(TableDetails {
        name: table_name,
        comment,
        columns,
        primary_keys,
        indexes,
        unique_constraints,
        foreign_keys,
        check_constraints,
        triggers,
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
    // DM8 stores identity column info in SYS.SYSCOLUMNS.INFO2 field
    // When INFO2 & 0x01 = 0x01, the column is an identity column
    // Use IDENT_SEED() and IDENT_INCR() functions to get seed and increment values
    // Note: DM8 allows only ONE identity column per table
    //
    // Length selection for string types:
    // - CHAR_USED = 'C' (CHAR semantics): use CHAR_LENGTH (character count)
    // - CHAR_USED = 'B' (BYTE semantics): use DATA_LENGTH (byte count)
    // - For non-string types: use DATA_LENGTH
    let sql = format!(
        "SELECT c.COLUMN_NAME, c.DATA_TYPE, \
                CASE WHEN c.DATA_TYPE IN ('CHAR','NCHAR','VARCHAR','VARCHAR2','NVARCHAR','NVARCHAR2') \
                          AND c.CHAR_USED = 'C' \
                     THEN c.CHAR_LENGTH \
                     ELSE c.DATA_LENGTH \
                END AS LENGTH, \
                c.DATA_PRECISION, c.DATA_SCALE, c.CHAR_USED, \
                c.NULLABLE, c.DATA_DEFAULT, \
                CASE WHEN sc.INFO2 & 1 = 1 THEN 'YES' ELSE 'NO' END AS IDENTITY_COLUMN, \
                cc.COMMENTS \
         FROM ALL_TAB_COLUMNS c \
         LEFT JOIN ALL_COL_COMMENTS cc ON cc.OWNER = c.OWNER AND cc.TABLE_NAME = c.TABLE_NAME AND cc.COLUMN_NAME = c.COLUMN_NAME \
         LEFT JOIN SYS.SYSOBJECTS sch ON sch.NAME = c.OWNER AND sch.TYPE$ = 'SCH' \
         LEFT JOIN SYS.SYSOBJECTS so ON so.NAME = c.TABLE_NAME AND so.SCHID = sch.ID AND so.TYPE$ = 'SCHOBJ' \
         LEFT JOIN SYS.SYSCOLUMNS sc ON sc.ID = so.ID AND sc.NAME = c.COLUMN_NAME \
         WHERE c.OWNER = '{}' AND c.TABLE_NAME = '{}' \
         ORDER BY c.COLUMN_ID",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = match connection.execute(&sql, ()).context("Failed to query DM8 columns")? {
        Some(cursor) => cursor,
        None => return Ok(vec![]),
    };

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))
        .context("Failed to prepare column buffer")?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut columns = Vec::new();

    while let Some(batch) = row_set_cursor.fetch().context("Failed to fetch column metadata")? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Encountered column without a name"))?
                .to_string();
            let data_type = batch.at_as_str(1, row_index)?
                .ok_or_else(|| anyhow!("Encountered column without data type"))?
                .to_string();
            let length = batch.at_as_str(2, row_index)?
                .and_then(|s| s.parse::<i32>().ok());
            let precision = batch.at_as_str(3, row_index)?.and_then(|s| s.parse::<i32>().ok());
            let scale = batch.at_as_str(4, row_index)?.and_then(|s| s.parse::<i32>().ok());
            let char_used = batch.at_as_str(5, row_index)?.map(|s| s.to_string());
            let nullable_flag = batch.at_as_str(6, row_index)?;
            let default_value = batch.at_as_str(7, row_index)?.map(|s| s.to_string());
            let identity_flag = batch.at_as_str(8, row_index)?;
            let comment = batch.at_as_str(9, row_index)?.map(|s| s.to_string());
            let nullable = matches!(nullable_flag, Some(flag) if flag.eq_ignore_ascii_case("Y"));
            let identity = matches!(identity_flag, Some(flag) if flag.eq_ignore_ascii_case("YES") || flag.eq_ignore_ascii_case("Y"));

            columns.push(Column {
                name,
                data_type,
                length,
                precision,
                scale,
                char_semantics: char_used,
                nullable,
                comment,
                default_value,
                identity,
                identity_start: None,
                identity_increment: None,
            });
        }
    }

    // Fetch identity seed and increment for tables with identity columns
    // Note: DM8 allows only ONE identity column per table, so we only update the first one found
    let has_identity = columns.iter().any(|c| c.identity);
    if has_identity {
        if let Ok(Some((seed, incr))) = fetch_identity_info(connection, schema, table) {
            // Only update the first identity column (DM8 constraint: one per table)
            if let Some(col) = columns.iter_mut().find(|c| c.identity) {
                col.identity_start = Some(seed);
                col.identity_increment = Some(incr);
            }
        }
    }

    Ok(columns)
}

fn fetch_identity_info(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Option<(i64, i64)>> {
    // Use IDENT_SEED and IDENT_INCR functions to get identity column properties
    // DM8 accepts table name in format: 'SCHEMA.TABLE' or '"SCHEMA"."TABLE"'
    let sql = format!(
        "SELECT IDENT_SEED('{}.{}'), IDENT_INCR('{}.{}') FROM DUAL",
        schema.replace("'", "''"),
        table.replace("'", "''"),
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = match connection.execute(&sql, ()).context("Failed to query identity info")? {
        Some(cursor) => cursor,
        None => return Ok(None),
    };

    let mut buffers = TextRowSet::for_cursor(1, &mut cursor, Some(64))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            let seed = batch.at_as_str(0, 0)?.and_then(|s| s.parse::<i64>().ok());
            let incr = batch.at_as_str(1, 0)?.and_then(|s| s.parse::<i64>().ok());
            if let (Some(seed), Some(incr)) = (seed, incr) {
                return Ok(Some((seed, incr)));
            }
        }
    }

    Ok(None)
}

const TRIGGER_LEVEL_FULL: u8 = 0;
const TRIGGER_LEVEL_NO_TYPE: u8 = 1;
const TRIGGER_LEVEL_NO_WHEN: u8 = 2;

fn is_trigger_metadata_missing(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let message = cause.to_string().to_uppercase();
        message.contains("TRIGGER_TYPE")
            || message.contains("WHEN_CLAUSE")
            || message.contains("TRIGGER_BODY")
            || message.contains("DESCRIPTION")
            || message.contains("42S22")
    })
}

fn trigger_missing_column(err: &anyhow::Error) -> Option<&'static str> {
    for cause in err.chain() {
        let message = cause.to_string().to_uppercase();
        if message.contains("TRIGGER_TYPE") {
            return Some("TRIGGER_TYPE");
        }
        if message.contains("WHEN_CLAUSE") {
            return Some("WHEN_CLAUSE");
        }
        if message.contains("DESCRIPTION") {
            return Some("DESCRIPTION");
        }
        if message.contains("TRIGGER_BODY") {
            return Some("TRIGGER_BODY");
        }
    }
    None
}

fn trigger_fallback_level(current_level: u8, err: &anyhow::Error) -> Option<u8> {
    let missing = trigger_missing_column(err);

    match (current_level, missing) {
        (TRIGGER_LEVEL_FULL, Some("TRIGGER_TYPE")) => Some(TRIGGER_LEVEL_NO_TYPE),
        (TRIGGER_LEVEL_FULL, Some("DESCRIPTION")) => Some(TRIGGER_LEVEL_NO_TYPE),
        (TRIGGER_LEVEL_FULL, Some("WHEN_CLAUSE")) => Some(TRIGGER_LEVEL_NO_WHEN),
        (TRIGGER_LEVEL_NO_TYPE, Some("WHEN_CLAUSE")) => Some(TRIGGER_LEVEL_NO_WHEN),
        (TRIGGER_LEVEL_NO_TYPE, Some("TRIGGER_TYPE")) => Some(TRIGGER_LEVEL_NO_TYPE),
        (TRIGGER_LEVEL_NO_TYPE, Some("DESCRIPTION")) => Some(TRIGGER_LEVEL_NO_TYPE),
        (TRIGGER_LEVEL_NO_WHEN, _) => None,
        (_, Some("TRIGGER_BODY")) => None,
        _ => {
            if is_trigger_metadata_missing(err) {
                match current_level {
                    TRIGGER_LEVEL_FULL => Some(TRIGGER_LEVEL_NO_TYPE),
                    TRIGGER_LEVEL_NO_TYPE => Some(TRIGGER_LEVEL_NO_WHEN),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}

pub fn fetch_row_count(connection: &Connection<'_>, schema: &str, table: &str) -> Result<i64> {
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

#[cfg(test)]
mod tests {
    use super::{is_trigger_metadata_missing, trigger_fallback_level};

    #[test]
    fn trigger_metadata_missing_detects_missing_trigger_type_column() {
        let err = anyhow::anyhow!(
            "State: 42S22, Native error: -2111, Message: 第1 行附近出现错误: 无效的列名[TRIGGER_TYPE]"
        );
        assert!(is_trigger_metadata_missing(&err));
    }

    #[test]
    fn trigger_metadata_missing_ignores_other_errors() {
        let err = anyhow::anyhow!("some other error");
        assert!(!is_trigger_metadata_missing(&err));
    }

    #[test]
    fn trigger_fallback_level_handles_missing_trigger_type() {
        let err = anyhow::anyhow!(
            "State: 42S22, Native error: -2111, Message: 第1 行附近出现错误: 无效的列名[TRIGGER_TYPE]"
        );
        assert_eq!(trigger_fallback_level(0, &err), Some(1));
    }

    #[test]
    fn trigger_fallback_level_handles_missing_when_clause() {
        let err = anyhow::anyhow!(
            "State: 42S22, Native error: -2111, Message: 第1 行附近出现错误: 无效的列名[WHEN_CLAUSE]"
        );
        assert_eq!(trigger_fallback_level(1, &err), Some(2));
    }
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

fn fetch_unique_constraints(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<UniqueConstraint>> {
    let sql = format!(
        "SELECT ac.CONSTRAINT_NAME, acc.COLUMN_NAME \
         FROM ALL_CONSTRAINTS ac \
         JOIN ALL_CONS_COLUMNS acc ON ac.OWNER = acc.OWNER AND ac.CONSTRAINT_NAME = acc.CONSTRAINT_NAME \
         WHERE ac.CONSTRAINT_TYPE = 'U' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME, acc.POSITION",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query unique constraints")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for unique constraint query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut constraints: Vec<UniqueConstraint> = Vec::new();
    let mut current_name: Option<String> = None;

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Unique constraint name missing"))?
                .to_string();
            let column = batch.at_as_str(1, row_index)?
                .ok_or_else(|| anyhow!("Unique constraint column missing"))?
                .to_string();

            if current_name.as_ref() != Some(&name) {
                constraints.push(UniqueConstraint {
                    name: name.clone(),
                    columns: vec![column],
                });
                current_name = Some(name);
            } else if let Some(last) = constraints.last_mut() {
                last.columns.push(column);
            }
        }
    }

    Ok(constraints)
}

fn fetch_check_constraints(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<CheckConstraint>> {
    let sql = format!(
        "SELECT ac.CONSTRAINT_NAME, ac.SEARCH_CONDITION \
         FROM ALL_CONSTRAINTS ac \
         WHERE ac.CONSTRAINT_TYPE = 'C' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query check constraints")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for check constraint query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut constraints = Vec::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Check constraint name missing"))?
                .to_string();
            let condition = batch.at_as_str(1, row_index)?
                .ok_or_else(|| anyhow!("Check constraint condition missing"))?
                .to_string();
            constraints.push(CheckConstraint { name, condition });
        }
    }

    Ok(constraints)
}

fn fetch_foreign_keys(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>> {
    // Try with UPDATE_RULE first, fallback without it if not supported
    // DM8 may not have UPDATE_RULE column in ALL_CONSTRAINTS
    let sql_with_update = format!(
        "SELECT ac.CONSTRAINT_NAME, ac.R_CONSTRAINT_NAME, ac.DELETE_RULE, ac.UPDATE_RULE \
         FROM ALL_CONSTRAINTS ac \
         WHERE ac.CONSTRAINT_TYPE = 'R' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let sql_without_update = format!(
        "SELECT ac.CONSTRAINT_NAME, ac.R_CONSTRAINT_NAME, ac.DELETE_RULE, NULL AS UPDATE_RULE \
         FROM ALL_CONSTRAINTS ac \
         WHERE ac.CONSTRAINT_TYPE = 'R' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    // Try with UPDATE_RULE first
    let (cursor_result, has_update_rule) = match connection.execute(&sql_with_update, ()) {
        Ok(cursor) => (Ok(cursor), true),
        Err(e) => {
            let err_msg = e.to_string().to_uppercase();
            if err_msg.contains("UPDATE_RULE") || err_msg.contains("-2207") {
                // UPDATE_RULE not supported, fallback
                (connection.execute(&sql_without_update, ()), false)
            } else {
                (Err(e), true)
            }
        }
    };

    let mut cursor = cursor_result
        .context("Failed to query foreign key constraints")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for foreign key constraint query"))?;

    if !has_update_rule {
        tracing::debug!("DM8 ALL_CONSTRAINTS does not have UPDATE_RULE column, using fallback query");
    }

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut fks = Vec::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Foreign key name missing"))?
                .to_string();
            let ref_constraint = batch.at_as_str(1, row_index)?
                .ok_or_else(|| anyhow!("Referenced constraint name missing"))?
                .to_string();
            let delete_rule = batch.at_as_str(2, row_index)?.map(|s| s.to_string());
            let update_rule = batch.at_as_str(3, row_index)?.map(|s| s.to_string());

            // Columns in FK
            let fk_cols = fetch_constraint_columns(connection, schema, &name)?;

            // Referenced table & columns
            let (ref_table, ref_cols) =
                fetch_referenced_columns(connection, &ref_constraint)?;

            fks.push(ForeignKey {
                name,
                columns: fk_cols,
                referenced_table: ref_table,
                referenced_columns: ref_cols,
                delete_rule,
                update_rule,
            });
        }
    }

    Ok(fks)
}

fn fetch_constraint_columns(
    connection: &Connection<'_>,
    schema: &str,
    constraint_name: &str,
) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT acc.COLUMN_NAME \
         FROM ALL_CONS_COLUMNS acc \
         WHERE acc.OWNER = '{}' AND acc.CONSTRAINT_NAME = '{}' \
         ORDER BY acc.POSITION",
        schema.replace("'", "''"),
        constraint_name.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query constraint columns")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for constraint columns query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut cols = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Constraint column missing"))?
                .to_string();
            cols.push(name);
        }
    }
    Ok(cols)
}

fn fetch_referenced_columns(
    connection: &Connection<'_>,
    referenced_constraint: &str,
) -> Result<(String, Vec<String>)> {
    let sql = format!(
        "SELECT ac.OWNER, ac.TABLE_NAME \
         FROM ALL_CONSTRAINTS ac \
         WHERE ac.CONSTRAINT_NAME = '{}'",
        referenced_constraint.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query referenced constraint")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for referenced constraint query"))?;

    let mut buffers = TextRowSet::for_cursor(10, &mut cursor, Some(128))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let (owner, table) = if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            let owner = batch.at_as_str(0, 0)?
                .ok_or_else(|| anyhow!("Referenced owner missing"))?
                .to_string();
            let table = batch.at_as_str(1, 0)?
                .ok_or_else(|| anyhow!("Referenced table missing"))?
                .to_string();
            (owner, table)
        } else {
            return Err(anyhow!("Referenced constraint {} not found", referenced_constraint));
        }
    } else {
        return Err(anyhow!("Referenced constraint {} not found", referenced_constraint));
    };

    let columns = fetch_constraint_columns(connection, &owner, referenced_constraint)?;
    Ok((format!("{}.{}", owner, table), columns))
}

pub fn fetch_sequences(connection: &Connection<'_>, schema: &str) -> Result<Vec<Sequence>> {
    let sql = format!(
        "SELECT SEQUENCE_NAME, MIN_VALUE, MAX_VALUE, INCREMENT_BY, CACHE_SIZE, CYCLE_FLAG, ORDER_FLAG, LAST_NUMBER \
         FROM ALL_SEQUENCES WHERE SEQUENCE_OWNER = '{}' ORDER BY SEQUENCE_NAME",
        schema.replace("'", "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query sequences")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for sequences query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut seqs = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Sequence name missing"))?
                .to_string();
            let min_value = batch.at_as_str(1, row_index)?.and_then(|s| s.parse::<i64>().ok());
            let max_value = batch.at_as_str(2, row_index)?.and_then(|s| s.parse::<i64>().ok());
            let increment_by = batch.at_as_str(3, row_index)?.and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
            let cache_size = batch.at_as_str(4, row_index)?.and_then(|s| s.parse::<i64>().ok());
            let cycle = matches!(batch.at_as_str(5, row_index)?, Some(v) if v.eq_ignore_ascii_case("Y"));
            let order = matches!(batch.at_as_str(6, row_index)?, Some(v) if v.eq_ignore_ascii_case("Y"));
            let last_number = batch.at_as_str(7, row_index)?.and_then(|s| s.parse::<i64>().ok());

            seqs.push(Sequence {
                name,
                min_value,
                max_value,
                increment_by,
                cache_size,
                cycle,
                order,
                start_with: last_number,
            });
        }
    }
    Ok(seqs)
}

fn fetch_triggers(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<TriggerDefinition>> {
    static TRIGGER_METADATA_LEVEL: AtomicU8 = AtomicU8::new(TRIGGER_LEVEL_FULL);

    let sql_full = format!(
        "SELECT TRIGGER_NAME, TRIGGER_TYPE, TRIGGERING_EVENT, TABLE_NAME, WHEN_CLAUSE, TRIGGER_BODY, DESCRIPTION \
         FROM ALL_TRIGGERS \
         WHERE TABLE_OWNER = '{}' AND TABLE_NAME = '{}' \
         ORDER BY TRIGGER_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let sql_no_type = format!(
        "SELECT TRIGGER_NAME, NULL AS TRIGGER_TYPE, TRIGGERING_EVENT, TABLE_NAME, WHEN_CLAUSE, TRIGGER_BODY, NULL AS DESCRIPTION \
         FROM ALL_TRIGGERS \
         WHERE TABLE_OWNER = '{}' AND TABLE_NAME = '{}' \
         ORDER BY TRIGGER_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let sql_no_when = format!(
        "SELECT TRIGGER_NAME, NULL AS TRIGGER_TYPE, TRIGGERING_EVENT, TABLE_NAME, NULL AS WHEN_CLAUSE, TRIGGER_BODY, NULL AS DESCRIPTION \
         FROM ALL_TRIGGERS \
         WHERE TABLE_OWNER = '{}' AND TABLE_NAME = '{}' \
         ORDER BY TRIGGER_NAME",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let trigger_level_label = |level| match level {
        TRIGGER_LEVEL_FULL => "full",
        TRIGGER_LEVEL_NO_TYPE => "no-trigger-type",
        TRIGGER_LEVEL_NO_WHEN => "no-when-clause",
        _ => "unknown",
    };

    let mut level = TRIGGER_METADATA_LEVEL.load(Ordering::Relaxed);
    let mut attempts = 0u8;
    let mut cursor = loop {
        let (sql, context_label) = match level {
            TRIGGER_LEVEL_FULL => (&sql_full, "Failed to query triggers (full)"),
            TRIGGER_LEVEL_NO_TYPE => (&sql_no_type, "Failed to query triggers (no trigger type)"),
            TRIGGER_LEVEL_NO_WHEN => (&sql_no_when, "Failed to query triggers (no when clause)"),
            _ => (&sql_no_when, "Failed to query triggers (fallback)"),
        };

        match connection.execute(sql, ()) {
            Ok(Some(cursor)) => break cursor,
            Ok(None) => return Ok(vec![]),
            Err(err) => {
                let err = anyhow!(err).context(context_label);
                if let Some(next_level) = trigger_fallback_level(level, &err) {
                    if next_level == level {
                        return Err(err);
                    }
                    attempts = attempts.saturating_add(1);
                    if attempts > 3 {
                        return Err(err);
                    }
                    if TRIGGER_METADATA_LEVEL
                        .compare_exchange(level, next_level, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                    {
                        level = next_level;
                    } else {
                        level = TRIGGER_METADATA_LEVEL.load(Ordering::Relaxed);
                    }
                    tracing::warn!(
                        "Trigger metadata not available, fallback to {}: {}",
                        trigger_level_label(level),
                        err
                    );
                    continue;
                }
                return Err(err);
            }
        }
    };

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut triggers = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = batch.at_as_str(0, row_index)?
                .ok_or_else(|| anyhow!("Trigger name missing"))?
                .to_string();
            let trigger_type = batch.at_as_str(1, row_index)?.unwrap_or("BEFORE");
            let triggering_event = batch.at_as_str(2, row_index)?.unwrap_or("INSERT");
            let table_name = batch.at_as_str(3, row_index)?.unwrap_or(table).to_string();
            let when_clause = batch.at_as_str(4, row_index)?.unwrap_or("").to_string();
            let body = batch.at_as_str(5, row_index)?.unwrap_or("").to_string();
            let description = batch.at_as_str(6, row_index)?.unwrap_or("").to_string();

            // DM8 uses " OR " as separator (e.g., "INSERT OR UPDATE OR DELETE")
            // Also support comma separator for compatibility
            let normalized_events = triggering_event.replace(" OR ", ",");
            let mut events: Vec<String> = normalized_events
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if events.is_empty() {
                events.push("INSERT".to_string());
            }

            // Extract timing from trigger_type (may contain "BEFORE EACH ROW", "AFTER STATEMENT", etc.)
            let trigger_type_upper = trigger_type.to_uppercase();
            let timing = if trigger_type_upper.contains("INSTEAD") {
                "INSTEAD OF".to_string()
            } else if trigger_type_upper.contains("AFTER") {
                "AFTER".to_string()
            } else {
                "BEFORE".to_string()
            };

            // Check for EACH ROW in both description and trigger_type
            let each_row = description.to_uppercase().contains("EACH ROW")
                || trigger_type_upper.contains("EACH ROW");

            let mut trigger_body = String::new();
            if !when_clause.trim().is_empty() {
                trigger_body.push_str(&format!("WHEN ({})\n", when_clause.trim()));
            }
            trigger_body.push_str(body.trim());

            triggers.push(TriggerDefinition {
                name,
                table_name,
                timing,
                events,
                each_row,
                body: trigger_body,
            });
        }
    }

    Ok(triggers)
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
