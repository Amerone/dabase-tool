use anyhow::{anyhow, ensure, Context, Result};
use odbc_api::{buffers::TextRowSet, Connection, Cursor};

use crate::db::pool::ConnectionPool;
use crate::models::{
    CheckConstraint, Column, ConnectionConfig, DbType, ForeignKey, Index, Sequence, Table,
    TableDetails, TriggerDefinition, UniqueConstraint,
};

fn ensure_kingbase(config: &ConnectionConfig) -> Result<()> {
    ensure!(
        matches!(config.db_type, DbType::Kingbase),
        "kingbase module only accepts DbType::Kingbase"
    );
    Ok(())
}

fn decode_cell(batch: &TextRowSet, col: usize, row: usize) -> Result<Option<String>> {
    match batch.at_as_str(col, row) {
        Ok(Some(s)) => Ok(Some(s.to_string())),
        Ok(None) => Ok(None),
        Err(e) => {
            // 尝试使用 UTF-8 解码
            match batch.at(col, row) {
                Some(bytes) => {
                    let (decoded, _encoding, had_errors) = encoding_rs::UTF_8.decode(bytes);
                    if had_errors {
                        Err(anyhow::anyhow!(
                            "Failed to decode cell at column {} row {}: {}",
                            col,
                            row,
                            e
                        ))
                    } else {
                        Ok(Some(decoded.into_owned()))
                    }
                }
                None => Ok(None),
            }
        }
    }
}

pub fn test_connection(_config: &ConnectionConfig) -> Result<()> {
    // Superseded by pg_native::test_connection — kept for API symmetry.
    Err(anyhow!(
        "KingBase test_connection: use pg_native::test_connection instead"
    ))
}

pub fn get_schemas(_config: &ConnectionConfig) -> Result<Vec<String>> {
    // Superseded by pg_native::get_schemas — kept for API symmetry.
    Err(anyhow!(
        "KingBase get_schemas: use pg_native::get_schemas instead"
    ))
}

pub fn get_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    ensure_kingbase(config)?;
    let pool = ConnectionPool::new(config.clone())?;
    let connection = pool.get_connection()?;

    let schema = config.schema.trim().replace('\'', "''");
    let sql = format!(
        "SELECT n.nspname AS schema_name, \
                c.relname AS table_name, \
                obj_description(c.oid) AS table_comment, \
                NULL AS row_count \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relkind = 'r' \
         ORDER BY c.relname",
        schema
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query KingBase tables")?
        .ok_or_else(|| anyhow!("KingBase returned no cursor for tables query"))?;

    let mut buffers = TextRowSet::for_cursor(200, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut tables = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let schema = decode_cell(batch, 0, row_index)?;
            let name = decode_cell(batch, 1, row_index)?
                .ok_or_else(|| anyhow!("Encountered table without a name"))?;
            let comment = decode_cell(batch, 2, row_index)?;
            tables.push(Table {
                schema,
                name,
                comment,
                row_count: None,
            });
        }
    }

    Ok(tables)
}

pub fn get_table_details(
    config: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    ensure_kingbase(config)?;
    let pool = ConnectionPool::new(config.clone())?;
    let connection = pool.get_connection()?;

    let schema_name = schema.trim();
    let table_name = table.trim();

    let columns = fetch_columns(&connection, schema_name, table_name)?;
    let primary_keys = fetch_primary_keys(&connection, schema_name, table_name)?;
    let indexes = fetch_indexes(&connection, schema_name, table_name)?;
    let unique_constraints = fetch_unique_constraints(&connection, schema_name, table_name)?;
    let foreign_keys = fetch_foreign_keys(&connection, schema_name, table_name)?;
    let check_constraints = fetch_check_constraints(&connection, schema_name, table_name)?;
    let triggers = fetch_triggers(&connection, schema_name, table_name)?;

    Ok(TableDetails {
        name: table_name.to_string(),
        comment: fetch_table_comment(&connection, schema, table_name).ok(),
        columns,
        primary_keys,
        indexes,
        unique_constraints,
        foreign_keys,
        check_constraints,
        triggers,
    })
}

fn fetch_table_comment(connection: &Connection<'_>, schema: &str, table: &str) -> Result<String> {
    let sql = format!(
        "SELECT obj_description((quote_ident('{}') || '.' || quote_ident('{}'))::regclass)",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query table comment")?
        .ok_or_else(|| anyhow!("No cursor for table comment query"))?;

    let mut buffers = TextRowSet::for_cursor(1, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    if let Some(batch) = row_set_cursor.fetch()? {
        if batch.num_rows() > 0 {
            if let Some(comment) = decode_cell(batch, 0, 0)? {
                return Ok(comment);
            }
        }
    }

    Ok(String::new())
}

fn fetch_columns(connection: &Connection<'_>, schema: &str, table: &str) -> Result<Vec<Column>> {
    let sql = format!(
        "SELECT a.attname AS column_name, \
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type, \
                a.attlen AS length, \
                CASE WHEN a.atttypid = ANY (ARRAY[1700]::oid[]) THEN ((a.atttypmod - 4) >> 16) & 65535 ELSE NULL END AS precision, \
                CASE WHEN a.atttypid = ANY (ARRAY[1700]::oid[]) THEN (a.atttypmod - 4) & 65535 ELSE NULL END AS scale, \
                NOT a.attnotnull AS is_nullable, \
                pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS column_default, \
                col_description(c.oid, a.attnum) AS column_comment \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON a.attrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query KingBase columns")?
        .ok_or_else(|| anyhow!("No cursor for columns query"))?;

    let mut buffers = TextRowSet::for_cursor(400, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut columns = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name =
                decode_cell(batch, 0, row_index)?.ok_or_else(|| anyhow!("Column without name"))?;
            let data_type = decode_cell(batch, 1, row_index)?.unwrap_or_default();
            let length = decode_cell(batch, 2, row_index)?.and_then(|s| s.parse::<i32>().ok());
            let precision = decode_cell(batch, 3, row_index)?.and_then(|s| s.parse::<i32>().ok());
            let scale = decode_cell(batch, 4, row_index)?.and_then(|s| s.parse::<i32>().ok());
            let nullable = decode_cell(batch, 5, row_index)?
                .map(|v| v.eq_ignore_ascii_case("t") || v.eq_ignore_ascii_case("true"))
                .unwrap_or(true);
            let default_value = decode_cell(batch, 6, row_index)?;
            let comment = decode_cell(batch, 7, row_index)?;

            columns.push(Column {
                name,
                data_type,
                length,
                precision,
                scale,
                char_semantics: None,
                nullable,
                comment,
                default_value,
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
        "SELECT a.attname \
         FROM pg_catalog.pg_constraint con \
         JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND con.contype = 'p' \
         ORDER BY array_position(con.conkey, a.attnum)",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query primary keys")?
        .ok_or_else(|| anyhow!("No cursor for primary keys query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut pk_columns = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            if let Some(col_name) = decode_cell(batch, 0, row_index)? {
                pk_columns.push(col_name);
            }
        }
    }

    Ok(pk_columns)
}

fn fetch_indexes(connection: &Connection<'_>, schema: &str, table: &str) -> Result<Vec<Index>> {
    let sql = format!(
        "SELECT i.relname AS index_name, \
                am.amname AS index_type, \
                ix.indisunique AS is_unique, \
                array_to_string(ARRAY(SELECT a.attname FROM pg_catalog.pg_attribute a WHERE a.attrelid = ix.indrelid AND a.attnum = ANY(ix.indkey) ORDER BY array_position(ix.indkey, a.attnum)), ',') AS columns \
         FROM pg_catalog.pg_index ix \
         JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_catalog.pg_class c ON c.oid = ix.indrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_am am ON am.oid = i.relam \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND NOT ix.indisprimary \
         ORDER BY i.relname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query indexes")?
        .ok_or_else(|| anyhow!("No cursor for indexes query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut indexes = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();
            let _index_type = decode_cell(batch, 1, row_index)?.unwrap_or_default();
            let is_unique = decode_cell(batch, 2, row_index)?
                .map(|v| v.eq_ignore_ascii_case("t") || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let columns_str = decode_cell(batch, 3, row_index)?.unwrap_or_default();
            let columns: Vec<String> = columns_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            indexes.push(Index {
                name,
                columns,
                unique: is_unique,
            });
        }
    }

    Ok(indexes)
}

fn fetch_unique_constraints(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<UniqueConstraint>> {
    let sql = format!(
        "SELECT con.conname AS constraint_name, \
                array_to_string(ARRAY(SELECT a.attname FROM pg_catalog.pg_attribute a WHERE a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey) ORDER BY array_position(con.conkey, a.attnum)), ',') AS columns \
         FROM pg_catalog.pg_constraint con \
         JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND con.contype = 'u' \
         ORDER BY con.conname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query unique constraints")?
        .ok_or_else(|| anyhow!("No cursor for unique constraints query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut constraints = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();
            let columns_str = decode_cell(batch, 1, row_index)?.unwrap_or_default();
            let columns: Vec<String> = columns_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            constraints.push(UniqueConstraint { name, columns });
        }
    }

    Ok(constraints)
}

fn fetch_foreign_keys(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>> {
    let sql = format!(
        "SELECT con.conname AS constraint_name, \
                array_to_string(ARRAY(SELECT a.attname FROM pg_catalog.pg_attribute a WHERE a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey) ORDER BY array_position(con.conkey, a.attnum)), ',') AS columns, \
                ref_ns.nspname AS ref_schema, \
                ref_c.relname AS ref_table, \
                array_to_string(ARRAY(SELECT a.attname FROM pg_catalog.pg_attribute a WHERE a.attrelid = con.confrelid AND a.attnum = ANY(con.confkey) ORDER BY array_position(con.confkey, a.attnum)), ',') AS ref_columns \
         FROM pg_catalog.pg_constraint con \
         JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_class ref_c ON con.confrelid = ref_c.oid \
         JOIN pg_catalog.pg_namespace ref_ns ON ref_ns.oid = ref_c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND con.contype = 'f' \
         ORDER BY con.conname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query foreign keys")?
        .ok_or_else(|| anyhow!("No cursor for foreign keys query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut fks = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();
            let columns_str = decode_cell(batch, 1, row_index)?.unwrap_or_default();
            let columns: Vec<String> = columns_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let ref_schema = decode_cell(batch, 2, row_index)?.unwrap_or_default();
            let ref_table = decode_cell(batch, 3, row_index)?.unwrap_or_default();
            let ref_columns_str = decode_cell(batch, 4, row_index)?.unwrap_or_default();
            let ref_columns: Vec<String> = ref_columns_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            fks.push(ForeignKey {
                name,
                columns,
                referenced_table: format!("{}.{}", ref_schema, ref_table),
                referenced_columns: ref_columns,
                delete_rule: None,
                update_rule: None,
            });
        }
    }

    Ok(fks)
}

fn fetch_check_constraints(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<CheckConstraint>> {
    let sql = format!(
        "SELECT con.conname AS constraint_name, \
                pg_catalog.pg_get_constraintdef(con.oid) AS check_clause \
         FROM pg_catalog.pg_constraint con \
         JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND con.contype = 'c' \
         ORDER BY con.conname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query check constraints")?
        .ok_or_else(|| anyhow!("No cursor for check constraints query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut checks = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();
            let check_clause = decode_cell(batch, 1, row_index)?.unwrap_or_default();

            checks.push(CheckConstraint {
                name,
                condition: check_clause,
            });
        }
    }

    Ok(checks)
}

fn fetch_triggers(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
) -> Result<Vec<TriggerDefinition>> {
    let sql = format!(
        "SELECT t.tgname AS trigger_name, \
                pg_catalog.pg_get_triggerdef(t.oid) AS trigger_definition \
         FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_class c ON t.tgrelid = c.oid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND NOT t.tgisinternal \
         ORDER BY t.tgname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query triggers")?
        .ok_or_else(|| anyhow!("No cursor for triggers query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(16384))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut triggers = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();
            let definition = decode_cell(batch, 1, row_index)?.unwrap_or_default();

            // Parse trigger definition to extract components
            // For now, store minimal info - full parsing can be added later
            triggers.push(TriggerDefinition {
                name: name.clone(),
                table_name: table.to_string(),
                timing: String::new(), // TODO: parse from definition
                events: vec![],        // TODO: parse from definition
                each_row: false,       // TODO: parse from definition
                body: definition,
            });
        }
    }

    Ok(triggers)
}

pub fn fetch_sequences(connection: &Connection<'_>, schema: &str) -> Result<Vec<Sequence>> {
    let sql = format!(
        "SELECT c.relname AS sequence_name, \
                NULL AS start_value, \
                NULL AS increment_by, \
                NULL AS min_value, \
                NULL AS max_value, \
                NULL AS cache_size, \
                NULL AS is_cycle \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relkind = 'S' \
         ORDER BY c.relname",
        schema.replace('\'', "''")
    );

    let mut cursor = connection
        .execute(&sql, ())
        .context("Failed to query sequences")?
        .ok_or_else(|| anyhow!("No cursor for sequences query"))?;

    let mut buffers = TextRowSet::for_cursor(100, &mut cursor, Some(4096))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    let mut sequences = Vec::new();
    while let Some(batch) = row_set_cursor.fetch()? {
        for row_index in 0..batch.num_rows() {
            let name = decode_cell(batch, 0, row_index)?.unwrap_or_default();

            sequences.push(Sequence {
                name,
                min_value: None,
                max_value: None,
                increment_by: 1,
                cache_size: None,
                cycle: false,
                order: false,
                start_with: None,
            });
        }
    }

    Ok(sequences)
}
