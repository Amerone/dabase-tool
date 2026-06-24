use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, ensure, Context, Result};
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::models::{
    CheckConstraint, Column, ForeignKey, Index, Sequence, Table, TableDetails, TriggerDefinition,
    UniqueConstraint,
};

/// Read a cell from a `TextRowSet` batch as an owned `String`.
///
/// DM8's Windows ODBC driver may return bytes in GBK / GB18030 even when
/// `CHARSET=1` is in the connection string (driver-version dependent).
/// We try UTF-8 first; on failure we re-decode with GB18030 so Chinese text
/// is never garbled or raises a hard error.
pub(crate) fn decode_cell(batch: &TextRowSet, col: usize, row: usize) -> Option<String> {
    match batch.at_as_str(col, row) {
        Ok(Some(s)) => Some(s.to_string()),
        Ok(None) => None,
        Err(_) => batch
            .at(col, row)
            .map(|bytes| encoding_rs::GB18030.decode(bytes).0.into_owned()),
    }
}

fn merge_duplicate_column(existing: &mut Column, duplicate: Column) {
    if existing.data_type.trim().is_empty() && !duplicate.data_type.trim().is_empty() {
        existing.data_type = duplicate.data_type;
    }

    existing.length = existing.length.or(duplicate.length);
    existing.precision = existing.precision.or(duplicate.precision);
    existing.scale = existing.scale.or(duplicate.scale);

    if existing
        .char_semantics
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        existing.char_semantics = duplicate.char_semantics;
    }

    existing.nullable &= duplicate.nullable;

    if existing
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        existing.comment = duplicate.comment;
    }

    if existing
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        existing.default_value = duplicate.default_value;
    }

    existing.identity |= duplicate.identity;
    existing.identity_start = existing.identity_start.or(duplicate.identity_start);
    existing.identity_increment = existing.identity_increment.or(duplicate.identity_increment);
}

fn deduplicate_columns(columns: Vec<Column>, table_name: &str) -> Vec<Column> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut deduplicated = Vec::with_capacity(columns.len());
    let mut duplicate_count = 0usize;

    for column in columns {
        let key = column.name.to_uppercase();
        if let Some(&index) = seen.get(&key) {
            merge_duplicate_column(&mut deduplicated[index], column);
            duplicate_count += 1;
            continue;
        }

        seen.insert(key, deduplicated.len());
        deduplicated.push(column);
    }

    if duplicate_count > 0 {
        tracing::warn!(
            table = table_name,
            duplicates = duplicate_count,
            "DM8 metadata returned duplicate columns; collapsing to unique column list"
        );
    }

    deduplicated
}

pub fn get_schemas(connection: &Connection<'_>) -> Result<Vec<String>> {
    let sql = "SELECT USERNAME FROM ALL_USERS ORDER BY USERNAME";

    let mut cursor = connection
        .execute(sql, ())
        .context("Failed to query DM8 schemas")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for schemas query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(2048))?;
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
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Encountered table without a name in DM8 metadata"))?;
            let comment = decode_cell(batch, 1, row_index);
            let row_count = decode_cell(batch, 2, row_index).and_then(|s| s.parse::<i64>().ok());

            tables.push(Table {
                schema: Some(owner.clone()),
                name,
                comment,
                row_count,
            });
        }
    }

    // 使用 ALL_TABLES.NUM_ROWS 统计值（可能略滞后于实际行数，但避免对每张表执行 COUNT(*)）

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
            let comment = decode_cell(batch, 0, 0);
            return Ok(comment);
        }
    }

    Ok(None)
}

fn fetch_columns(connection: &Connection<'_>, schema: &str, table: &str) -> Result<Vec<Column>> {
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

    let mut cursor = match connection
        .execute(&sql, ())
        .context("Failed to query DM8 columns")?
    {
        Some(cursor) => cursor,
        None => return Ok(vec![]),
    };

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))
        .context("Failed to prepare column buffer")?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut columns = Vec::new();

    while let Some(batch) = row_set_cursor
        .fetch()
        .context("Failed to fetch column metadata")?
    {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Encountered column without a name"))?;
            let data_type = decode_cell(batch, 1, row_index)
                .ok_or_else(|| anyhow!("Encountered column without data type"))?;
            let length = decode_cell(batch, 2, row_index).and_then(|s| s.parse::<i32>().ok());
            let precision = decode_cell(batch, 3, row_index).and_then(|s| s.parse::<i32>().ok());
            let scale = decode_cell(batch, 4, row_index).and_then(|s| s.parse::<i32>().ok());
            let char_used = decode_cell(batch, 5, row_index);
            let nullable_flag = decode_cell(batch, 6, row_index);
            let default_value = decode_cell(batch, 7, row_index);
            let identity_flag = decode_cell(batch, 8, row_index);
            let comment = decode_cell(batch, 9, row_index);
            let nullable =
                matches!(nullable_flag, Some(ref flag) if flag.eq_ignore_ascii_case("Y"));
            let identity = matches!(identity_flag, Some(ref flag) if flag.eq_ignore_ascii_case("YES") || flag.eq_ignore_ascii_case("Y"));

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

    let mut columns = deduplicate_columns(columns, table);

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

    let mut cursor = match connection
        .execute(&sql, ())
        .context("Failed to query identity info")?
    {
        Some(cursor) => cursor,
        None => return Ok(None),
    };

    let mut buffers = TextRowSet::for_cursor(1, &mut cursor, Some(64))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            let seed = decode_cell(batch, 0, 0).and_then(|s| s.parse::<i64>().ok());
            let incr = decode_cell(batch, 1, 0).and_then(|s| s.parse::<i64>().ok());
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
            if let Some(val) = decode_cell(batch, 0, 0) {
                if let Ok(count) = val.parse::<i64>() {
                    return Ok(count);
                }
            }
        }
    }

    Err(anyhow!("Failed to read row count for {}", table))
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::models::{Column, TableDetails};

    use super::{deduplicate_columns, is_trigger_metadata_missing, trigger_fallback_level};

    fn sample_column(name: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: "VARCHAR".to_string(),
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
        }
    }

    #[test]
    fn deduplicate_columns_collapses_duplicate_names_and_merges_metadata() {
        let mut duplicate_a = sample_column("files");
        duplicate_a.comment = Some("file metadata".to_string());

        let mut duplicate_b = sample_column("FILES");
        duplicate_b.nullable = false;
        duplicate_b.default_value = Some("'[]'".to_string());
        duplicate_b.identity = true;
        duplicate_b.identity_start = Some(1);
        duplicate_b.identity_increment = Some(1);

        let columns = deduplicate_columns(
            vec![sample_column("id"), duplicate_a, duplicate_b],
            "PUBLIC_INTERFACE_TO_CONFIG",
        );

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[1].name, "files");
        assert_eq!(columns[1].comment.as_deref(), Some("file metadata"));
        assert_eq!(columns[1].default_value.as_deref(), Some("'[]'"));
        assert!(!columns[1].nullable);
        assert!(columns[1].identity);
        assert_eq!(columns[1].identity_start, Some(1));
        assert_eq!(columns[1].identity_increment, Some(1));
    }

    #[test]
    fn metadata_query_table_names_includes_exact_and_uppercase_names() {
        let names = super::metadata_query_table_names(&[
            "business_filling".to_string(),
            "PLATFORM_USER".to_string(),
        ]);

        assert_eq!(
            names,
            vec![
                "business_filling".to_string(),
                "BUSINESS_FILLING".to_string(),
                "PLATFORM_USER".to_string()
            ]
        );
    }

    #[test]
    fn remove_selected_table_details_prefers_exact_case_match() {
        let mut map = HashMap::from([
            (
                "business_filling".to_string(),
                TableDetails {
                    name: "business_filling".to_string(),
                    comment: None,
                    columns: vec![sample_column("id")],
                    primary_keys: vec![],
                    indexes: vec![],
                    unique_constraints: vec![],
                    foreign_keys: vec![],
                    check_constraints: vec![],
                    triggers: vec![],
                },
            ),
            (
                "BUSINESS_FILLING".to_string(),
                TableDetails {
                    name: "BUSINESS_FILLING".to_string(),
                    comment: None,
                    columns: vec![sample_column("ID")],
                    primary_keys: vec![],
                    indexes: vec![],
                    unique_constraints: vec![],
                    foreign_keys: vec![],
                    check_constraints: vec![],
                    triggers: vec![],
                },
            ),
        ]);

        let details = super::remove_selected_table_details(&mut map, "business_filling").unwrap();

        assert_eq!(details.name, "business_filling");
    }

    #[test]
    fn remove_complete_selected_table_details_rejects_empty_column_metadata() {
        let mut map = HashMap::from([(
            "TEST_ASSOCIATION_20260624135807355".to_string(),
            TableDetails {
                name: "TEST_ASSOCIATION_20260624135807355".to_string(),
                comment: None,
                columns: vec![],
                primary_keys: vec![],
                indexes: vec![],
                unique_constraints: vec![],
                foreign_keys: vec![],
                check_constraints: vec![],
                triggers: vec![],
            },
        )]);

        let details = super::remove_complete_selected_table_details(
            &mut map,
            "TEST_ASSOCIATION_20260624135807355",
        );

        assert!(details.is_none());
    }

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
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Primary key column name missing"))?;
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
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Unique constraint name missing"))?;
            let column = decode_cell(batch, 1, row_index)
                .ok_or_else(|| anyhow!("Unique constraint column missing"))?;

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
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Check constraint name missing"))?;
            let condition = decode_cell(batch, 1, row_index)
                .ok_or_else(|| anyhow!("Check constraint condition missing"))?;
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
    #[derive(Default)]
    struct FkInfo {
        columns: Vec<String>,
        referenced_constraint: String,
        referenced_owner: Option<String>,
        delete_rule: Option<String>,
        update_rule: Option<String>,
    }

    let make_ref_key =
        |owner: &str, constraint: &str| format!("{}|{}", owner.trim(), constraint.trim());

    // Try with UPDATE_RULE first, fallback without it if not supported
    // DM8 may not have UPDATE_RULE column in ALL_CONSTRAINTS
    let sql_with_update = format!(
        "SELECT ac.CONSTRAINT_NAME, ac.R_CONSTRAINT_NAME, ac.R_OWNER, ac.DELETE_RULE, ac.UPDATE_RULE, acc.COLUMN_NAME \
         FROM ALL_CONSTRAINTS ac \
         JOIN ALL_CONS_COLUMNS acc ON ac.OWNER = acc.OWNER AND ac.CONSTRAINT_NAME = acc.CONSTRAINT_NAME \
         WHERE ac.CONSTRAINT_TYPE = 'R' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME, acc.POSITION",
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let sql_without_update = format!(
        "SELECT ac.CONSTRAINT_NAME, ac.R_CONSTRAINT_NAME, ac.R_OWNER, ac.DELETE_RULE, NULL AS UPDATE_RULE, acc.COLUMN_NAME \
         FROM ALL_CONSTRAINTS ac \
         JOIN ALL_CONS_COLUMNS acc ON ac.OWNER = acc.OWNER AND ac.CONSTRAINT_NAME = acc.CONSTRAINT_NAME \
         WHERE ac.CONSTRAINT_TYPE = 'R' AND ac.OWNER = '{}' AND ac.TABLE_NAME = '{}' \
         ORDER BY ac.CONSTRAINT_NAME, acc.POSITION",
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
        tracing::debug!(
            "DM8 ALL_CONSTRAINTS does not have UPDATE_RULE column, using fallback query"
        );
    }

    let mut buffers = TextRowSet::for_cursor(1000, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut fk_map: HashMap<String, FkInfo> = HashMap::new();

    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Foreign key name missing"))?;
            let referenced_constraint = decode_cell(batch, 1, row_index)
                .ok_or_else(|| anyhow!("Referenced constraint name missing"))?;
            let referenced_owner = decode_cell(batch, 2, row_index);
            let delete_rule = decode_cell(batch, 3, row_index);
            let update_rule = decode_cell(batch, 4, row_index);
            let column = decode_cell(batch, 5, row_index)
                .ok_or_else(|| anyhow!("Foreign key column missing"))?;

            let entry = fk_map.entry(name).or_insert_with(|| FkInfo {
                referenced_constraint,
                referenced_owner,
                delete_rule,
                update_rule,
                ..Default::default()
            });
            entry.columns.push(column);
        }
    }

    let mut referenced_conditions = Vec::new();
    let mut seen_references = HashSet::new();
    for fk in fk_map.values() {
        let Some(owner) = fk
            .referenced_owner
            .as_deref()
            .map(str::trim)
            .filter(|owner| !owner.is_empty())
        else {
            continue;
        };

        let constraint = fk.referenced_constraint.trim();
        if constraint.is_empty() {
            continue;
        }

        if seen_references.insert(make_ref_key(owner, constraint)) {
            referenced_conditions.push(format!(
                "(acc.OWNER = '{}' AND acc.CONSTRAINT_NAME = '{}')",
                owner.replace("'", "''"),
                constraint.replace("'", "''")
            ));
        }
    }

    let mut referenced_map: HashMap<String, (String, Vec<String>)> = HashMap::new();
    if !referenced_conditions.is_empty() {
        let sql = format!(
            "SELECT acc.OWNER, acc.CONSTRAINT_NAME, ac.TABLE_NAME, acc.COLUMN_NAME \
             FROM ALL_CONS_COLUMNS acc \
             JOIN ALL_CONSTRAINTS ac ON acc.OWNER = ac.OWNER AND acc.CONSTRAINT_NAME = ac.CONSTRAINT_NAME \
             WHERE {} \
             ORDER BY acc.OWNER, acc.CONSTRAINT_NAME, acc.POSITION",
            referenced_conditions.join(" OR ")
        );

        let mut cursor = connection
            .execute(&sql, ())
            .context("Failed to query referenced foreign key columns")?
            .ok_or_else(|| {
                anyhow!("DM8 returned no cursor for referenced foreign key columns query")
            })?;

        let mut buffers = TextRowSet::for_cursor(1000, &mut cursor, Some(8192))?;
        let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

        while let Some(batch) = row_set_cursor.fetch()? {
            for row_index in 0..batch.num_rows() {
                let owner = decode_cell(batch, 0, row_index)
                    .ok_or_else(|| anyhow!("Referenced owner missing"))?;
                let constraint = decode_cell(batch, 1, row_index)
                    .ok_or_else(|| anyhow!("Referenced constraint name missing"))?;
                let table = decode_cell(batch, 2, row_index)
                    .ok_or_else(|| anyhow!("Referenced table missing"))?;
                let column = decode_cell(batch, 3, row_index)
                    .ok_or_else(|| anyhow!("Referenced column missing"))?;

                let entry = referenced_map
                    .entry(make_ref_key(&owner, &constraint))
                    .or_insert_with(|| (format!("{}.{}", owner, table), Vec::new()));
                entry.1.push(column);
            }
        }
    }

    let mut fks = Vec::with_capacity(fk_map.len());
    for (name, fk) in fk_map {
        let referenced = fk
            .referenced_owner
            .as_deref()
            .map(str::trim)
            .filter(|owner| !owner.is_empty())
            .map(|owner| make_ref_key(owner, &fk.referenced_constraint))
            .and_then(|key| referenced_map.get(&key).cloned());

        let (ref_table, ref_cols) = match referenced {
            Some(referenced) => referenced,
            None => fetch_referenced_columns(
                connection,
                fk.referenced_owner.as_deref(),
                &fk.referenced_constraint,
            )?,
        };

        fks.push(ForeignKey {
            name,
            columns: fk.columns,
            referenced_table: ref_table,
            referenced_columns: ref_cols,
            delete_rule: fk.delete_rule,
            update_rule: fk.update_rule,
        });
    }
    fks.sort_by(|a, b| a.name.cmp(&b.name));

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
            let name = decode_cell(batch, 0, row_index)
                .ok_or_else(|| anyhow!("Constraint column missing"))?;
            cols.push(name);
        }
    }
    Ok(cols)
}

fn fetch_referenced_columns(
    connection: &Connection<'_>,
    referenced_owner: Option<&str>,
    referenced_constraint: &str,
) -> Result<(String, Vec<String>)> {
    let sql = if let Some(owner) = referenced_owner
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
    {
        format!(
            "SELECT ac.OWNER, ac.TABLE_NAME \
             FROM ALL_CONSTRAINTS ac \
             WHERE ac.OWNER = '{}' AND ac.CONSTRAINT_NAME = '{}'",
            owner.replace("'", "''"),
            referenced_constraint.replace("'", "''")
        )
    } else {
        format!(
            "SELECT ac.OWNER, ac.TABLE_NAME \
             FROM ALL_CONSTRAINTS ac \
             WHERE ac.CONSTRAINT_NAME = '{}'",
            referenced_constraint.replace("'", "''")
        )
    };

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query referenced constraint")?
        .ok_or_else(|| anyhow!("DM8 returned no cursor for referenced constraint query"))?;

    let mut buffers = TextRowSet::for_cursor(10, &mut cursor, Some(128))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let (owner, table) = if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            let owner =
                decode_cell(batch, 0, 0).ok_or_else(|| anyhow!("Referenced owner missing"))?;
            let table =
                decode_cell(batch, 1, 0).ok_or_else(|| anyhow!("Referenced table missing"))?;
            (owner, table)
        } else {
            return Err(anyhow!(
                "Referenced constraint {} not found",
                referenced_constraint
            ));
        }
    } else {
        return Err(anyhow!(
            "Referenced constraint {} not found",
            referenced_constraint
        ));
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
            let name =
                decode_cell(batch, 0, row_index).ok_or_else(|| anyhow!("Sequence name missing"))?;
            let min_value = decode_cell(batch, 1, row_index).and_then(|s| s.parse::<i64>().ok());
            let max_value = decode_cell(batch, 2, row_index).and_then(|s| s.parse::<i64>().ok());
            let increment_by = decode_cell(batch, 3, row_index)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(1);
            let cache_size = decode_cell(batch, 4, row_index).and_then(|s| s.parse::<i64>().ok());
            let cycle = matches!(decode_cell(batch, 5, row_index), Some(ref v) if v.eq_ignore_ascii_case("Y"));
            let order = matches!(decode_cell(batch, 6, row_index), Some(ref v) if v.eq_ignore_ascii_case("Y"));
            let last_number = decode_cell(batch, 7, row_index).and_then(|s| s.parse::<i64>().ok());

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

    let mut level = TRIGGER_LEVEL_FULL;
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
                    level = next_level;
                    tracing::warn!(
                        "Trigger metadata not available for this request, fallback to {}: {}",
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
            let name =
                decode_cell(batch, 0, row_index).ok_or_else(|| anyhow!("Trigger name missing"))?;
            let trigger_type =
                decode_cell(batch, 1, row_index).unwrap_or_else(|| "BEFORE".to_string());
            let triggering_event =
                decode_cell(batch, 2, row_index).unwrap_or_else(|| "INSERT".to_string());
            let table_name = decode_cell(batch, 3, row_index).unwrap_or_else(|| table.to_string());
            let when_clause = decode_cell(batch, 4, row_index).unwrap_or_default();
            let body = decode_cell(batch, 5, row_index).unwrap_or_default();
            let description = decode_cell(batch, 6, row_index).unwrap_or_default();

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
                || trigger_type_upper.contains("EACH ROW")
                || body.to_uppercase().contains(":NEW.")
                || body.to_uppercase().contains(":OLD.")
                || when_clause.to_uppercase().contains("NEW.");

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
fn fetch_indexes(connection: &Connection<'_>, schema: &str, table: &str) -> Result<Vec<Index>> {
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
            let name =
                decode_cell(batch, 0, row_index).ok_or_else(|| anyhow!("Index name missing"))?;
            let uniqueness = decode_cell(batch, 1, row_index);
            let unique = matches!(
                uniqueness,
                Some(ref flag) if flag.eq_ignore_ascii_case("UNIQUE") || flag.eq_ignore_ascii_case("Y")
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
        None => {
            return Ok(order
                .into_iter()
                .filter_map(|name| indexes.remove(&name))
                .collect())
        }
    };

    let mut col_buffers = TextRowSet::for_cursor(100, &mut column_cursor, Some(8192))?;
    let mut col_row_set_cursor = column_cursor.bind_buffer(&mut col_buffers)?;

    while let Some(batch) = col_row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let index_name = match decode_cell(batch, 0, row_index) {
                Some(val) => val,
                None => continue,
            };
            let column_name = match decode_cell(batch, 1, row_index) {
                Some(val) => val,
                None => continue,
            };

            if let Some(index) = indexes.get_mut(&index_name) {
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

// ──────────────────────────────────────────────────────────────────────────────
// Batch metadata fetch — replaces N×8 serial queries with ~9 bulk queries
// ──────────────────────────────────────────────────────────────────────────────

fn make_in_clause(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("'{}'", n.replace("'", "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/**
* AI generated 2026-06-24 16:57:36
* Author: 梁国栋
* Version: v1.0.0
* Function: Build and consume the DM8 metadata table-name set with both exact and uppercase names so quoted lowercase tables are not lost in batch metadata export.
* Revision history:
* - 2026-06-24 16:57:36 v1.0.0 Added exact-name metadata matching for case-sensitive DM8 tables, reviewer: 梁国栋
*/
fn metadata_query_table_names(table_names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut query_names = Vec::with_capacity(table_names.len() * 2);

    for table_name in table_names {
        let exact_name = table_name.trim();
        if !exact_name.is_empty() && seen.insert(exact_name.to_string()) {
            query_names.push(exact_name.to_string());
        }

        let upper_name = exact_name.to_uppercase();
        if !upper_name.is_empty() && seen.insert(upper_name.clone()) {
            query_names.push(upper_name);
        }
    }

    query_names
}

fn remove_selected_table_details(
    map: &mut HashMap<String, TableDetails>,
    selected_name: &str,
) -> Option<TableDetails> {
    let exact_name = selected_name.trim();
    map.remove(exact_name)
        .or_else(|| map.remove(&exact_name.to_uppercase()))
}

fn remove_complete_selected_table_details(
    map: &mut HashMap<String, TableDetails>,
    selected_name: &str,
) -> Option<TableDetails> {
    remove_selected_table_details(map, selected_name).filter(|details| !details.columns.is_empty())
}

/**
* AI generated 2026-06-24 17:54:40
* Author: 梁国栋
* Version: v1.0.0
* Function: Fill gaps left by DM8 batch metadata queries with the existing per-table metadata path so one missed table does not fail a full export.
* Revision history:
* - 2026-06-24 17:54:40 v1.0.0 Added single-table metadata fallback for batch query misses, reviewer: 梁国栋
*/
fn complete_missing_table_details(
    connection: &Connection<'_>,
    schema: &str,
    table_names: &[String],
    mut map: HashMap<String, TableDetails>,
) -> Result<Vec<TableDetails>> {
    let mut result = Vec::with_capacity(table_names.len());

    for table_name in table_names {
        if let Some(details) = remove_complete_selected_table_details(&mut map, table_name) {
            result.push(details);
            continue;
        }

        tracing::warn!(
            schema = schema,
            table = table_name.as_str(),
            "Batch DM8 metadata query missed selected table; retrying with single-table metadata query"
        );
        let details = get_table_details(connection, schema, table_name).with_context(|| {
            format!(
                "Failed to fetch fallback metadata for selected table '{}.{}'",
                schema, table_name
            )
        })?;
        result.push(details);
    }

    Ok(result)
}

/// Fetch full `TableDetails` for multiple tables in one shot.
///
/// Uses `IN (…)` bulk queries instead of a per-table round-trip for each
/// metadata category, reducing total ODBC queries from O(N) to a constant ~9.
pub fn get_tables_details_batch(
    connection: &Connection<'_>,
    schema: &str,
    table_names: &[String],
) -> Result<Vec<TableDetails>> {
    if table_names.is_empty() {
        return Ok(vec![]);
    }

    let owner = schema.to_uppercase();
    let query_names = metadata_query_table_names(table_names);
    let in_clause = make_in_clause(&query_names);

    // Seed result map in original order
    let mut map: HashMap<String, TableDetails> = query_names
        .iter()
        .map(|n| {
            (
                n.clone(),
                TableDetails {
                    name: n.clone(),
                    comment: None,
                    columns: vec![],
                    primary_keys: vec![],
                    indexes: vec![],
                    unique_constraints: vec![],
                    foreign_keys: vec![],
                    check_constraints: vec![],
                    triggers: vec![],
                },
            )
        })
        .collect();

    tracing::info!(
        "get_tables_details_batch: start ({} tables)",
        table_names.len()
    );

    // ── 1. Table comments ──────────────────────────────────────────────────
    {
        let sql = format!(
            "SELECT TABLE_NAME, COMMENTS FROM ALL_TAB_COMMENTS \
             WHERE OWNER = '{}' AND TABLE_NAME IN ({})",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let Some(tname) = decode_cell(batch, 0, row) {
                        if let Some(entry) = map.get_mut(&tname) {
                            entry.comment =
                                decode_cell(batch, 1, row).filter(|s| !s.trim().is_empty());
                        }
                    }
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [1/8] table comments done");

    // ── 2. Columns + column comments ──────────────────────────────────────
    {
        let sql = format!(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, \
                    CASE WHEN c.DATA_TYPE IN ('CHAR','NCHAR','VARCHAR','VARCHAR2','NVARCHAR','NVARCHAR2') \
                              AND c.CHAR_USED = 'C' \
                         THEN c.CHAR_LENGTH ELSE c.DATA_LENGTH END AS LEN, \
                    c.DATA_PRECISION, c.DATA_SCALE, c.CHAR_USED, c.NULLABLE, c.DATA_DEFAULT, \
                    CASE WHEN sc.INFO2 & 1 = 1 THEN 'YES' ELSE 'NO' END AS IS_IDENTITY, \
                    cc.COMMENTS \
             FROM ALL_TAB_COLUMNS c \
             LEFT JOIN ALL_COL_COMMENTS cc ON cc.OWNER=c.OWNER AND cc.TABLE_NAME=c.TABLE_NAME AND cc.COLUMN_NAME=c.COLUMN_NAME \
             LEFT JOIN SYS.SYSOBJECTS sch ON sch.NAME=c.OWNER AND sch.TYPE$='SCH' \
             LEFT JOIN SYS.SYSOBJECTS so  ON so.NAME=c.TABLE_NAME AND so.SCHID=sch.ID AND so.TYPE$='SCHOBJ' \
             LEFT JOIN SYS.SYSCOLUMNS sc  ON sc.ID=so.ID AND sc.NAME=c.COLUMN_NAME \
             WHERE c.OWNER = '{}' AND c.TABLE_NAME IN ({}) \
             ORDER BY c.TABLE_NAME, c.COLUMN_ID",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    let tname = match decode_cell(batch, 0, row) {
                        Some(n) => n,
                        None => continue,
                    };
                    let entry = match map.get_mut(&tname) {
                        Some(e) => e,
                        None => continue,
                    };
                    let name = match decode_cell(batch, 1, row) {
                        Some(n) => n,
                        None => continue,
                    };
                    let data_type = decode_cell(batch, 2, row).unwrap_or_default();
                    let length = decode_cell(batch, 3, row).and_then(|s| s.parse::<i32>().ok());
                    let precision = decode_cell(batch, 4, row).and_then(|s| s.parse::<i32>().ok());
                    let scale = decode_cell(batch, 5, row).and_then(|s| s.parse::<i32>().ok());
                    let char_semantics = decode_cell(batch, 6, row);
                    let nullable = matches!(
                        decode_cell(batch, 7, row),
                        Some(ref f) if f.eq_ignore_ascii_case("Y")
                    );
                    let default_value = decode_cell(batch, 8, row);
                    let identity = matches!(
                        decode_cell(batch, 9, row),
                        Some(ref f) if f.eq_ignore_ascii_case("YES") || f.eq_ignore_ascii_case("Y")
                    );
                    let comment = decode_cell(batch, 10, row);
                    entry.columns.push(Column {
                        name,
                        data_type,
                        length,
                        precision,
                        scale,
                        char_semantics,
                        nullable,
                        comment,
                        default_value,
                        identity,
                        identity_start: None,
                        identity_increment: None,
                    });
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [2/8] columns done");

    for entry in map.values_mut() {
        entry.columns = deduplicate_columns(std::mem::take(&mut entry.columns), &entry.name);
    }

    // ── 2b. Identity seed/increment (per-table, only for tables that need it) ─
    for entry in map.values_mut() {
        if entry.columns.iter().any(|c| c.identity) {
            if let Ok(Some((seed, incr))) = fetch_identity_info(connection, &owner, &entry.name) {
                if let Some(col) = entry.columns.iter_mut().find(|c| c.identity) {
                    col.identity_start = Some(seed);
                    col.identity_increment = Some(incr);
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [2b/8] identity info done");

    // ── 3. Primary keys ───────────────────────────────────────────────────
    {
        let sql = format!(
            "SELECT ac.TABLE_NAME, acc.COLUMN_NAME \
             FROM ALL_CONSTRAINTS ac \
             JOIN ALL_CONS_COLUMNS acc ON ac.OWNER=acc.OWNER AND ac.CONSTRAINT_NAME=acc.CONSTRAINT_NAME \
             WHERE ac.CONSTRAINT_TYPE='P' AND ac.OWNER='{}' AND ac.TABLE_NAME IN ({}) \
             ORDER BY ac.TABLE_NAME, acc.POSITION",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let (Some(tname), Some(col)) =
                        (decode_cell(batch, 0, row), decode_cell(batch, 1, row))
                    {
                        if let Some(entry) = map.get_mut(&tname) {
                            entry.primary_keys.push(col);
                        }
                    }
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [3/8] primary keys done");

    // ── 4. Indexes (name + uniqueness) ────────────────────────────────────
    {
        // Temporary per-table index maps for column assembly
        let mut idx_meta: HashMap<String, HashMap<String, (bool, Vec<String>)>> = HashMap::new();

        let sql = format!(
            "SELECT ai.TABLE_NAME, ai.INDEX_NAME, ai.UNIQUENESS \
             FROM ALL_INDEXES ai \
             WHERE ai.TABLE_OWNER='{}' AND ai.TABLE_NAME IN ({}) \
             ORDER BY ai.TABLE_NAME, ai.INDEX_NAME",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let (Some(tname), Some(iname)) =
                        (decode_cell(batch, 0, row), decode_cell(batch, 1, row))
                    {
                        let unique = matches!(
                            decode_cell(batch, 2, row),
                            Some(ref f) if f.eq_ignore_ascii_case("UNIQUE") || f.eq_ignore_ascii_case("Y")
                        );
                        idx_meta
                            .entry(tname)
                            .or_default()
                            .entry(iname)
                            .or_insert((unique, vec![]));
                    }
                }
            }
        }

        // Index columns
        let sql = format!(
            "SELECT ic.TABLE_NAME, ic.INDEX_NAME, ic.COLUMN_NAME \
             FROM ALL_IND_COLUMNS ic \
             WHERE ic.INDEX_OWNER='{}' AND ic.TABLE_NAME IN ({}) \
             ORDER BY ic.TABLE_NAME, ic.INDEX_NAME, ic.COLUMN_POSITION",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(1000, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let (Some(tname), Some(iname), Some(col)) = (
                        decode_cell(batch, 0, row),
                        decode_cell(batch, 1, row),
                        decode_cell(batch, 2, row),
                    ) {
                        if let Some(tmap) = idx_meta.get_mut(&tname) {
                            if let Some(entry) = tmap.get_mut(&iname) {
                                entry.1.push(col);
                            }
                        }
                    }
                }
            }
        }

        // Populate map
        for (tname, imap) in idx_meta {
            if let Some(entry) = map.get_mut(&tname) {
                for (iname, (unique, columns)) in imap {
                    if !columns.is_empty() {
                        entry.indexes.push(Index {
                            name: iname,
                            columns,
                            unique,
                        });
                    }
                }
                entry.indexes.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }
    }

    tracing::info!("get_tables_details_batch: [4/8] indexes done");

    // ── 5. Unique constraints ─────────────────────────────────────────────
    {
        let sql = format!(
            "SELECT ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.COLUMN_NAME \
             FROM ALL_CONSTRAINTS ac \
             JOIN ALL_CONS_COLUMNS acc ON ac.OWNER=acc.OWNER AND ac.CONSTRAINT_NAME=acc.CONSTRAINT_NAME \
             WHERE ac.CONSTRAINT_TYPE='U' AND ac.OWNER='{}' AND ac.TABLE_NAME IN ({}) \
             ORDER BY ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.POSITION",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let (Some(tname), Some(cname), Some(col)) = (
                        decode_cell(batch, 0, row),
                        decode_cell(batch, 1, row),
                        decode_cell(batch, 2, row),
                    ) {
                        if let Some(entry) = map.get_mut(&tname) {
                            if let Some(uc) = entry
                                .unique_constraints
                                .iter_mut()
                                .find(|u| u.name == cname)
                            {
                                uc.columns.push(col);
                            } else {
                                entry.unique_constraints.push(UniqueConstraint {
                                    name: cname,
                                    columns: vec![col],
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [5/8] unique constraints done");

    // ── 6. Check constraints ──────────────────────────────────────────────
    {
        let sql = format!(
            "SELECT ac.TABLE_NAME, ac.CONSTRAINT_NAME, ac.SEARCH_CONDITION \
             FROM ALL_CONSTRAINTS ac \
             WHERE ac.CONSTRAINT_TYPE='C' AND ac.OWNER='{}' AND ac.TABLE_NAME IN ({}) \
             ORDER BY ac.TABLE_NAME, ac.CONSTRAINT_NAME",
            owner.replace("'", "''"),
            in_clause
        );
        if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
            let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(8192))?;
            let mut rs = cursor.bind_buffer(&mut buffers)?;
            while let Some(batch) = rs.fetch()? {
                for row in 0..batch.num_rows() {
                    if let (Some(tname), Some(cname), Some(cond)) = (
                        decode_cell(batch, 0, row),
                        decode_cell(batch, 1, row),
                        decode_cell(batch, 2, row),
                    ) {
                        if let Some(entry) = map.get_mut(&tname) {
                            entry.check_constraints.push(CheckConstraint {
                                name: cname,
                                condition: cond,
                            });
                        }
                    }
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [6/8] check constraints done");

    // ── 7. Foreign keys (2 queries instead of N×3) ────────────────────────
    {
        // Query 1: FK constraints + FK columns
        #[derive(Default)]
        struct FkInfo {
            columns: Vec<String>,
            r_constraint: String,
            r_owner: Option<String>,
            delete_rule: Option<String>,
            update_rule: Option<String>,
        }
        let make_ref_key =
            |owner: &str, constraint: &str| format!("{}|{}", owner.trim(), constraint.trim());
        // table → constraint_name → FkInfo
        let mut fk_map: HashMap<String, HashMap<String, FkInfo>> = HashMap::new();

        let sql_with_update = format!(
            "SELECT ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.COLUMN_NAME, \
                    ac.R_CONSTRAINT_NAME, ac.R_OWNER, ac.DELETE_RULE, ac.UPDATE_RULE \
             FROM ALL_CONSTRAINTS ac \
             JOIN ALL_CONS_COLUMNS acc ON ac.OWNER=acc.OWNER AND ac.CONSTRAINT_NAME=acc.CONSTRAINT_NAME \
             WHERE ac.CONSTRAINT_TYPE='R' AND ac.OWNER='{}' AND ac.TABLE_NAME IN ({}) \
             ORDER BY ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.POSITION",
            owner.replace("'", "''"),
            in_clause
        );

        let sql_without_update = format!(
            "SELECT ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.COLUMN_NAME, \
                    ac.R_CONSTRAINT_NAME, ac.R_OWNER, ac.DELETE_RULE, NULL AS UPDATE_RULE \
             FROM ALL_CONSTRAINTS ac \
             JOIN ALL_CONS_COLUMNS acc ON ac.OWNER=acc.OWNER AND ac.CONSTRAINT_NAME=acc.CONSTRAINT_NAME \
             WHERE ac.CONSTRAINT_TYPE='R' AND ac.OWNER='{}' AND ac.TABLE_NAME IN ({}) \
             ORDER BY ac.TABLE_NAME, ac.CONSTRAINT_NAME, acc.POSITION",
            owner.replace("'", "''"),
            in_clause
        );

        let (cursor_result, has_update_rule) = match connection.execute(&sql_with_update, ()) {
            Ok(cursor) => (Ok(cursor), true),
            Err(err) => {
                let message = err.to_string().to_uppercase();
                if message.contains("UPDATE_RULE") || message.contains("-2207") {
                    (connection.execute(&sql_without_update, ()), false)
                } else {
                    (Err(err), true)
                }
            }
        };

        if !has_update_rule {
            tracing::debug!(
                "DM8 ALL_CONSTRAINTS does not have UPDATE_RULE column, using batch FK fallback query"
            );
        }

        match cursor_result {
            Ok(Some(mut cursor)) => {
                let mut buffers = TextRowSet::for_cursor(1000, &mut cursor, Some(8192))?;
                let mut rs = cursor.bind_buffer(&mut buffers)?;
                while let Some(batch) = rs.fetch()? {
                    for row in 0..batch.num_rows() {
                        let tname = match decode_cell(batch, 0, row) {
                            Some(v) => v,
                            None => continue,
                        };
                        let cname = match decode_cell(batch, 1, row) {
                            Some(v) => v,
                            None => continue,
                        };
                        let col = match decode_cell(batch, 2, row) {
                            Some(v) => v,
                            None => continue,
                        };
                        let r_constraint = decode_cell(batch, 3, row).unwrap_or_default();
                        let r_owner = decode_cell(batch, 4, row);
                        let delete_rule = decode_cell(batch, 5, row);
                        let update_rule = decode_cell(batch, 6, row);

                        let fk = fk_map
                            .entry(tname)
                            .or_default()
                            .entry(cname)
                            .or_insert_with(|| FkInfo {
                                r_constraint: r_constraint.clone(),
                                r_owner: r_owner.clone(),
                                delete_rule: delete_rule.clone(),
                                update_rule: update_rule.clone(),
                                ..Default::default()
                            });
                        fk.columns.push(col);
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    "Batch foreign key query failed, skipping foreign keys: {}",
                    err
                );
            }
        }

        // Collect all unique referenced constraint names for batch lookup
        let mut ref_conditions = Vec::new();
        let mut seen_refs = HashSet::new();
        for fk in fk_map.values().flat_map(|fks| fks.values()) {
            let Some(owner) = fk
                .r_owner
                .as_deref()
                .map(str::trim)
                .filter(|owner| !owner.is_empty())
            else {
                continue;
            };
            let constraint = fk.r_constraint.trim();
            if constraint.is_empty() {
                continue;
            }

            if seen_refs.insert(make_ref_key(owner, constraint)) {
                ref_conditions.push(format!(
                    "(acc.OWNER='{}' AND acc.CONSTRAINT_NAME='{}')",
                    owner.replace("'", "''"),
                    constraint.replace("'", "''")
                ));
            }
        }

        if !ref_conditions.is_empty() {
            // Query 2: referenced table + columns for all referenced constraints at once
            let sql = format!(
                "SELECT acc.OWNER, acc.CONSTRAINT_NAME, ac.TABLE_NAME, acc.COLUMN_NAME \
                 FROM ALL_CONS_COLUMNS acc \
                 JOIN ALL_CONSTRAINTS ac ON acc.OWNER=ac.OWNER AND acc.CONSTRAINT_NAME=ac.CONSTRAINT_NAME \
                 WHERE {} \
                 ORDER BY acc.OWNER, acc.CONSTRAINT_NAME, acc.POSITION",
                ref_conditions.join(" OR ")
            );

            // ref_constraint_name → (ref_owner.ref_table, Vec<col>)
            let mut ref_map: HashMap<String, (String, Vec<String>)> = HashMap::new();
            if let Ok(Some(mut cursor)) = connection.execute(&sql, ()) {
                let mut buffers = TextRowSet::for_cursor(1000, &mut cursor, Some(8192))?;
                let mut rs = cursor.bind_buffer(&mut buffers)?;
                while let Some(batch) = rs.fetch()? {
                    for row in 0..batch.num_rows() {
                        if let (Some(rowner), Some(cname), Some(tname), Some(col)) = (
                            decode_cell(batch, 0, row),
                            decode_cell(batch, 1, row),
                            decode_cell(batch, 2, row),
                            decode_cell(batch, 3, row),
                        ) {
                            let entry = ref_map
                                .entry(make_ref_key(&rowner, &cname))
                                .or_insert_with(|| (format!("{}.{}", rowner, tname), vec![]));
                            entry.1.push(col);
                        }
                    }
                }
            }

            // Populate FKs into each table's entry
            for (tname, fks) in fk_map {
                if let Some(entry) = map.get_mut(&tname) {
                    let mut fk_list: Vec<ForeignKey> = fks
                        .into_iter()
                        .filter_map(|(cname, fk)| {
                            let (ref_table, ref_cols) = fk
                                .r_owner
                                .as_deref()
                                .map(str::trim)
                                .filter(|owner| !owner.is_empty())
                                .map(|owner| make_ref_key(owner, &fk.r_constraint))
                                .and_then(|key| ref_map.get(&key).cloned())
                                .or_else(|| {
                                    fetch_referenced_columns(
                                        connection,
                                        fk.r_owner.as_deref(),
                                        &fk.r_constraint,
                                    )
                                    .ok()
                                })?;
                            Some(ForeignKey {
                                name: cname,
                                columns: fk.columns,
                                referenced_table: ref_table,
                                referenced_columns: ref_cols,
                                delete_rule: fk.delete_rule,
                                update_rule: fk.update_rule,
                            })
                        })
                        .collect();
                    fk_list.sort_by(|a, b| a.name.cmp(&b.name));
                    entry.foreign_keys = fk_list;
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [7/8] foreign keys done");

    // ── 8. Triggers ───────────────────────────────────────────────────────
    {
        let sql_full = format!(
            "SELECT TABLE_NAME, TRIGGER_NAME, TRIGGER_TYPE, TRIGGERING_EVENT, \
                    WHEN_CLAUSE, TRIGGER_BODY, DESCRIPTION \
             FROM ALL_TRIGGERS \
             WHERE TABLE_OWNER='{}' AND TABLE_NAME IN ({}) \
             ORDER BY TABLE_NAME, TRIGGER_NAME",
            owner.replace("'", "''"),
            in_clause
        );
        let sql_no_type = format!(
            "SELECT TABLE_NAME, TRIGGER_NAME, NULL AS TRIGGER_TYPE, TRIGGERING_EVENT, \
                    WHEN_CLAUSE, TRIGGER_BODY, NULL AS DESCRIPTION \
             FROM ALL_TRIGGERS \
             WHERE TABLE_OWNER='{}' AND TABLE_NAME IN ({}) \
             ORDER BY TABLE_NAME, TRIGGER_NAME",
            owner.replace("'", "''"),
            in_clause
        );
        let sql_no_when = format!(
            "SELECT TABLE_NAME, TRIGGER_NAME, NULL AS TRIGGER_TYPE, TRIGGERING_EVENT, \
                    NULL AS WHEN_CLAUSE, TRIGGER_BODY, NULL AS DESCRIPTION \
             FROM ALL_TRIGGERS \
             WHERE TABLE_OWNER='{}' AND TABLE_NAME IN ({}) \
             ORDER BY TABLE_NAME, TRIGGER_NAME",
            owner.replace("'", "''"),
            in_clause
        );

        let trigger_sqls = [sql_full, sql_no_type, sql_no_when];
        let mut level: usize = 0;
        'trig: loop {
            match connection.execute(&trigger_sqls[level], ()) {
                Ok(None) => break,
                Ok(Some(mut cursor)) => {
                    let mut buffers = TextRowSet::for_cursor(500, &mut cursor, Some(32768))?;
                    let mut rs = cursor.bind_buffer(&mut buffers)?;
                    while let Some(batch) = rs.fetch()? {
                        for row in 0..batch.num_rows() {
                            let tname = match decode_cell(batch, 0, row) {
                                Some(n) => n,
                                None => continue,
                            };
                            let entry = match map.get_mut(&tname) {
                                Some(e) => e,
                                None => continue,
                            };
                            let tr_name = decode_cell(batch, 1, row).unwrap_or_default();
                            let trigger_type =
                                decode_cell(batch, 2, row).unwrap_or_else(|| "BEFORE".to_string());
                            let triggering_event =
                                decode_cell(batch, 3, row).unwrap_or_else(|| "INSERT".to_string());
                            let when_clause = decode_cell(batch, 4, row).unwrap_or_default();
                            let body = decode_cell(batch, 5, row).unwrap_or_default();
                            let description = decode_cell(batch, 6, row).unwrap_or_default();

                            let normalized = triggering_event.replace(" OR ", ",");
                            let mut events: Vec<String> = normalized
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            if events.is_empty() {
                                events.push("INSERT".to_string());
                            }

                            let ttu = trigger_type.to_uppercase();
                            let timing = if ttu.contains("INSTEAD") {
                                "INSTEAD OF".to_string()
                            } else if ttu.contains("AFTER") {
                                "AFTER".to_string()
                            } else {
                                "BEFORE".to_string()
                            };
                            let each_row = description.to_uppercase().contains("EACH ROW")
                                || ttu.contains("EACH ROW")
                                || body.to_uppercase().contains(":NEW.")
                                || body.to_uppercase().contains(":OLD.")
                                || when_clause.to_uppercase().contains("NEW.");

                            let mut trigger_body = String::new();
                            if !when_clause.trim().is_empty() {
                                trigger_body.push_str(&format!("WHEN ({})\n", when_clause.trim()));
                            }
                            trigger_body.push_str(body.trim());

                            entry.triggers.push(TriggerDefinition {
                                name: tr_name,
                                table_name: tname.clone(),
                                timing,
                                events,
                                each_row,
                                body: trigger_body,
                            });
                        }
                    }
                    break 'trig;
                }
                Err(err) => {
                    let err = anyhow!(err);
                    if level < 2 && is_trigger_metadata_missing(&err) {
                        level += 1;
                        tracing::warn!("Batch trigger query fallback to level {}: {}", level, err);
                        continue;
                    }
                    // Non-fatal: skip triggers on error
                    tracing::warn!("Batch trigger query failed, skipping triggers: {}", err);
                    break;
                }
            }
        }
    }

    tracing::info!("get_tables_details_batch: [8/8] triggers done");

    complete_missing_table_details(connection, &owner, table_names, map)
}
