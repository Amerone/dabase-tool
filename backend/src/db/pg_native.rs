/// Native PostgreSQL connector for KingBase (and any PostgreSQL-wire-compatible DB).
///
/// Uses `tokio-postgres` directly so we bypass the Windows ODBC Driver Manager entirely.
/// KingBase V8R6 speaks the PostgreSQL wire protocol, so this works reliably.
use anyhow::{Context, Result};
use tokio_postgres::{Client, NoTls};

use crate::models::{
    CheckConstraint, Column, ConnectionConfig, ForeignKey, Index, Sequence, Table, TableDetails,
    TriggerDefinition, UniqueConstraint,
};

// ── connection helpers ────────────────────────────────────────────────────────

pub async fn connect(config: &ConnectionConfig) -> Result<Client> {
    let host = config.host.trim();
    let port = config.port;
    let user = config.username.trim();
    let password = &config.password;
    let dbname = config.schema.trim();

    let conn_str = if dbname.is_empty() {
        format!(
            "host={} port={} user={} password={}",
            host, port, user, password
        )
    } else {
        format!(
            "host={} port={} user={} password={} dbname={}",
            host, port, user, password, dbname
        )
    };

    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
        .await
        .with_context(|| {
            format!(
                "Failed to connect to KingBase at {}:{} as {}",
                host, port, user
            )
        })?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::warn!("KingBase pg_native connection error: {}", e);
        }
    });

    Ok(client)
}

async fn connect_without_db(config: &ConnectionConfig) -> Result<Client> {
    let host = config.host.trim();
    let port = config.port;
    let user = config.username.trim();
    let password = &config.password;

    let conn_str = format!(
        "host={} port={} user={} password={}",
        host, port, user, password
    );

    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
        .await
        .with_context(|| {
            format!(
                "Failed to connect to KingBase at {}:{} as {} (no db)",
                host, port, user
            )
        })?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::warn!("KingBase pg_native (no-db) connection error: {}", e);
        }
    });

    Ok(client)
}

// ── public API ────────────────────────────────────────────────────────────────

pub async fn test_connection(config: &ConnectionConfig) -> Result<()> {
    let client = connect(config).await?;
    client
        .execute("SELECT 1", &[])
        .await
        .context("Connected to KingBase but health query failed")?;
    Ok(())
}

pub async fn get_schemas(config: &ConnectionConfig) -> Result<Vec<String>> {
    let client = connect_without_db(config).await?;
    let rows = client
        .query(
            "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name",
            &[],
        )
        .await
        .context("Failed to query KingBase schemata")?;

    Ok(rows.into_iter().map(|r| r.get::<_, String>(0)).collect())
}

pub async fn get_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    let client = connect(config).await?;

    // config.schema is used as the PostgreSQL *database* name for connection.
    // Within that database, table ownership belongs to PostgreSQL schemas (namespaces),
    // most commonly "public". We list all user tables across all non-system schemas.
    let rows = client
        .query(
            "SELECT n.nspname AS schema_name, c.relname AS table_name, obj_description(c.oid) AS table_comment \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind = 'r' \
               AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') \
               AND n.nspname NOT LIKE 'pg_temp%' \
               AND n.nspname NOT LIKE 'pg_toast_temp%' \
             ORDER BY n.nspname, c.relname",
            &[],
        )
        .await
        .context("Failed to query KingBase tables")?;

    let tables = rows
        .into_iter()
        .map(|r| Table {
            schema: Some(r.get(0)),
            name: r.get(1),
            comment: r.get(2),
            row_count: None,
        })
        .collect();

    Ok(tables)
}

pub async fn get_table_details(
    config: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let client = connect(config).await?;
    let table = table.trim().to_string();
    let schema = schema.trim().to_string();

    let comment = fetch_table_comment(&client, &schema, &table).await.ok();
    let columns = fetch_columns(&client, &schema, &table).await?;
    let primary_keys = fetch_primary_keys(&client, &schema, &table).await?;
    let indexes = fetch_indexes(&client, &schema, &table).await?;
    let unique_constraints = fetch_unique_constraints(&client, &schema, &table).await?;
    let foreign_keys = fetch_foreign_keys(&client, &schema, &table).await?;
    let check_constraints = fetch_check_constraints(&client, &schema, &table).await?;
    let triggers = fetch_triggers(&client, &schema, &table).await?;

    Ok(TableDetails {
        name: table,
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

/// Resolve the actual pg_namespace name that owns the given table.
/// Prefers user schemas over system schemas.
pub async fn resolve_table_schema(client: &Client, table: &str) -> Result<String> {
    let rows = client
        .query(
            "SELECT n.nspname \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relname = $1 AND c.relkind = 'r' \
               AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') \
               AND n.nspname NOT LIKE 'pg_temp%' \
             ORDER BY n.nspname \
             LIMIT 1",
            &[&table],
        )
        .await
        .context("Failed to resolve schema for table")?;

    rows.into_iter()
        .next()
        .map(|r| r.get::<_, String>(0))
        .ok_or_else(|| anyhow::anyhow!("Table '{}' not found in any user schema", table))
}

pub async fn fetch_sequences(config: &ConnectionConfig) -> Result<Vec<Sequence>> {
    let client = connect(config).await?;
    let schema = config.schema.trim().to_string();

    let rows = client
        .query(
            "SELECT c.relname AS sequence_name \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind = 'S' \
             ORDER BY c.relname",
            &[&schema],
        )
        .await
        .context("Failed to query KingBase sequences")?;

    let sequences = rows
        .into_iter()
        .map(|r| Sequence {
            name: r.get(0),
            min_value: None,
            max_value: None,
            increment_by: 1,
            cache_size: None,
            cycle: false,
            order: false,
            start_with: None,
        })
        .collect();

    Ok(sequences)
}

// ── internal helpers ──────────────────────────────────────────────────────────

async fn fetch_table_comment(client: &Client, schema: &str, table: &str) -> Result<String> {
    let rows = client
        .query(
            "SELECT obj_description(\
               (quote_ident($1) || '.' || quote_ident($2))::regclass\
             )",
            &[&schema, &table],
        )
        .await
        .context("Failed to query table comment")?;

    if let Some(row) = rows.first() {
        if let Some(comment) = row.get::<_, Option<String>>(0) {
            return Ok(comment);
        }
    }
    Ok(String::new())
}

async fn fetch_columns(client: &Client, schema: &str, table: &str) -> Result<Vec<Column>> {
    let rows = client
        .query(
            "SELECT a.attname, \
                    pg_catalog.format_type(a.atttypid, a.atttypmod), \
                    a.attlen, \
                    CASE WHEN a.atttypid = ANY (ARRAY[1700]::oid[]) \
                         THEN ((a.atttypmod - 4) >> 16) & 65535 ELSE NULL END, \
                    CASE WHEN a.atttypid = ANY (ARRAY[1700]::oid[]) \
                         THEN (a.atttypmod - 4) & 65535 ELSE NULL END, \
                    NOT a.attnotnull, \
                    pg_catalog.pg_get_expr(d.adbin, d.adrelid), \
                    col_description(c.oid, a.attnum) \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON a.attrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_catalog.pg_attrdef d \
               ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
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
        // attlen is int4 in pg_catalog; -1 means variable-length
        let raw_len: i32 = row.get(2);
        let length: Option<i32> = if raw_len > 0 { Some(raw_len) } else { None };
        let precision: Option<i64> = row.get(3);
        let scale: Option<i64> = row.get(4);
        let nullable: bool = row.get(5);
        let default_value: Option<String> = row.get(6);
        let comment: Option<String> = row.get(7);

        columns.push(Column {
            name,
            data_type,
            length,
            precision: precision.map(|v| v as i32),
            scale: scale.map(|v| v as i32),
            char_semantics: None,
            nullable,
            comment,
            default_value,
            identity: false,
            identity_start: None,
            identity_increment: None,
        });
    }

    Ok(columns)
}

async fn fetch_primary_keys(client: &Client, schema: &str, table: &str) -> Result<Vec<String>> {
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

async fn fetch_indexes(client: &Client, schema: &str, table: &str) -> Result<Vec<Index>> {
    let rows = client
        .query(
            "SELECT i.relname, \
                    ix.indisunique, \
                    array_to_string(ARRAY(\
                      SELECT a.attname FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = ix.indrelid AND a.attnum = ANY(ix.indkey) \
                      ORDER BY array_position(ix.indkey, a.attnum)\
                    ), ',') \
             FROM pg_catalog.pg_index ix \
             JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
             JOIN pg_catalog.pg_class c ON c.oid = ix.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND NOT ix.indisprimary \
             ORDER BY i.relname",
            &[&schema, &table],
        )
        .await
        .context("Failed to query indexes")?;

    let indexes = rows
        .into_iter()
        .map(|r| {
            let name: String = r.get(0);
            let unique: bool = r.get(1);
            let cols_str: String = r.get(2);
            let columns = cols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Index {
                name,
                columns,
                unique,
            }
        })
        .collect();

    Ok(indexes)
}

async fn fetch_unique_constraints(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<UniqueConstraint>> {
    let rows = client
        .query(
            "SELECT con.conname, \
                    array_to_string(ARRAY(\
                      SELECT a.attname FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey) \
                      ORDER BY array_position(con.conkey, a.attnum)\
                    ), ',') \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'u' \
             ORDER BY con.conname",
            &[&schema, &table],
        )
        .await
        .context("Failed to query unique constraints")?;

    let constraints = rows
        .into_iter()
        .map(|r| {
            let name: String = r.get(0);
            let cols_str: String = r.get(1);
            let columns = cols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            UniqueConstraint { name, columns }
        })
        .collect();

    Ok(constraints)
}

pub async fn fetch_foreign_keys(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>> {
    let rows = client
        .query(
            "SELECT con.conname, \
                    array_to_string(ARRAY(\
                      SELECT a.attname FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey) \
                      ORDER BY array_position(con.conkey, a.attnum)\
                    ), ','), \
                    ref_ns.nspname, \
                    ref_c.relname, \
                    array_to_string(ARRAY(\
                      SELECT a.attname FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = con.confrelid AND a.attnum = ANY(con.confkey) \
                      ORDER BY array_position(con.confkey, a.attnum)\
                    ), ',') \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_class ref_c ON con.confrelid = ref_c.oid \
             JOIN pg_catalog.pg_namespace ref_ns ON ref_ns.oid = ref_c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'f' \
             ORDER BY con.conname",
            &[&schema, &table],
        )
        .await
        .context("Failed to query foreign keys")?;

    let fks = rows
        .into_iter()
        .map(|r| {
            let name: String = r.get(0);
            let cols_str: String = r.get(1);
            let columns = cols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let ref_schema: String = r.get(2);
            let ref_table: String = r.get(3);
            let ref_cols_str: String = r.get(4);
            let referenced_columns = ref_cols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            ForeignKey {
                name,
                columns,
                referenced_table: format!("{}.{}", ref_schema, ref_table),
                referenced_columns,
                delete_rule: None,
                update_rule: None,
            }
        })
        .collect();

    Ok(fks)
}

async fn fetch_check_constraints(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<CheckConstraint>> {
    let rows = client
        .query(
            "SELECT con.conname, pg_catalog.pg_get_constraintdef(con.oid) \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'c' \
             ORDER BY con.conname",
            &[&schema, &table],
        )
        .await
        .context("Failed to query check constraints")?;

    Ok(rows
        .into_iter()
        .map(|r| CheckConstraint {
            name: r.get(0),
            condition: r.get(1),
        })
        .collect())
}

async fn fetch_triggers(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<TriggerDefinition>> {
    let rows = client
        .query(
            "SELECT t.tgname, pg_catalog.pg_get_triggerdef(t.oid) \
             FROM pg_catalog.pg_trigger t \
             JOIN pg_catalog.pg_class c ON t.tgrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND NOT t.tgisinternal \
             ORDER BY t.tgname",
            &[&schema, &table],
        )
        .await
        .context("Failed to query triggers")?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let name: String = r.get(0);
            let body: String = r.get(1);
            TriggerDefinition {
                name: name.clone(),
                table_name: table.to_string(),
                timing: String::new(),
                events: vec![],
                each_row: false,
                body,
            }
        })
        .collect())
}
