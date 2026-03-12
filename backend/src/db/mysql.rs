use anyhow::{Context, Result};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{ConnectOptions, Row};
use std::collections::HashMap;
use tracing::warn;

use crate::models::{
    CheckConstraint, Column, ConnectionConfig, ForeignKey, Index, Table, TableDetails,
    TriggerDefinition, UniqueConstraint,
};

fn connect_options(config: &ConnectionConfig) -> MySqlConnectOptions {
    let mut options = MySqlConnectOptions::new()
        .host(config.host.trim())
        .port(config.port)
        .username(config.username.trim())
        .password(&config.password)
        .disable_statement_logging();

    let schema = config.schema.trim();
    if !schema.is_empty() {
        options = options.database(schema);
    }

    options
}

fn validate_without_schema(config: &ConnectionConfig) -> Result<()> {
    anyhow::ensure!(!config.host.trim().is_empty(), "Database host is required");
    anyhow::ensure!(config.port > 0, "Database port must be greater than zero");
    anyhow::ensure!(
        !config.username.trim().is_empty(),
        "Database username is required"
    );
    anyhow::ensure!(!config.password.is_empty(), "Database password is required");
    Ok(())
}

pub async fn create_pool(
    config: &ConnectionConfig,
    max_connections: u32,
) -> Result<sqlx::MySqlPool> {
    config
        .validate()
        .context("Invalid MySQL connection configuration")?;

    MySqlPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(connect_options(config))
        .await
        .context("Failed to connect to MySQL")
}

async fn create_pool_without_schema(
    config: &ConnectionConfig,
    max_connections: u32,
) -> Result<sqlx::MySqlPool> {
    validate_without_schema(config).context("Invalid MySQL connection configuration")?;

    MySqlPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(connect_options(config))
        .await
        .context("Failed to connect to MySQL")
}

pub async fn test_connection(config: &ConnectionConfig) -> Result<()> {
    let pool = create_pool(config, 1).await?;

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .context("Connected to MySQL but failed health query")?;

    pool.close().await;
    Ok(())
}

pub async fn get_schemas(config: &ConnectionConfig) -> Result<Vec<String>> {
    let pool = create_pool_without_schema(config, 1).await?;

    let rows = sqlx::query(
        "SELECT SCHEMA_NAME \
         FROM information_schema.SCHEMATA \
         ORDER BY SCHEMA_NAME",
    )
    .fetch_all(&pool)
    .await
    .context("Failed to query MySQL schemata")?;

    let schemas = rows
        .into_iter()
        .map(|row| row.get::<String, _>("SCHEMA_NAME"))
        .collect::<Vec<_>>();

    pool.close().await;
    Ok(schemas)
}

pub async fn get_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    let pool = create_pool(config, 2).await?;

    let rows = sqlx::query(
        "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_COMMENT, TABLE_ROWS \
         FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? \
         ORDER BY TABLE_NAME",
    )
    .bind(config.schema.trim())
    .fetch_all(&pool)
    .await
    .context("Failed to query MySQL tables")?;

    let tables = rows
        .into_iter()
        .map(|row| {
            let comment: Option<String> = row.try_get("TABLE_COMMENT").ok();
            let row_count_u64: Option<u64> = row.try_get("TABLE_ROWS").ok();
            Table {
                schema: row.try_get::<String, _>("TABLE_SCHEMA").ok(),
                name: row.get::<String, _>("TABLE_NAME"),
                comment: comment.filter(|v| !v.trim().is_empty()),
                row_count: row_count_u64.and_then(|v| i64::try_from(v).ok()),
            }
        })
        .collect();

    pool.close().await;
    Ok(tables)
}

pub async fn get_table_details(config: &ConnectionConfig, schema: &str, table: &str) -> Result<TableDetails> {
    let table_name = table.trim();
    let schema_name = schema.trim();
    let pool = create_pool(config, 2).await?;

    let comment_row = sqlx::query(
        "SELECT TABLE_COMMENT FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? LIMIT 1",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_optional(&pool)
    .await
    .context("Failed to query table comment")?;

    let comment = comment_row.and_then(|row| {
        let value: Option<String> = row.try_get("TABLE_COMMENT").ok();
        value.filter(|v| !v.trim().is_empty())
    });

    let column_rows = sqlx::query(
        "SELECT COLUMN_NAME, COLUMN_TYPE, CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, \
                IS_NULLABLE, COLUMN_DEFAULT, COLUMN_COMMENT, EXTRA \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
         ORDER BY ORDINAL_POSITION",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(&pool)
    .await
    .context("Failed to query MySQL columns")?;

    let columns = column_rows
        .into_iter()
        .map(|row| {
            let nullable = row
                .try_get::<String, _>("IS_NULLABLE")
                .map(|v| v.eq_ignore_ascii_case("YES"))
                .unwrap_or(true);
            let extra = row
                .try_get::<String, _>("EXTRA")
                .unwrap_or_default()
                .to_lowercase();

            Column {
                name: row.get::<String, _>("COLUMN_NAME"),
                data_type: row.get::<String, _>("COLUMN_TYPE"),
                length: row
                    .try_get::<Option<u64>, _>("CHARACTER_MAXIMUM_LENGTH")
                    .ok()
                    .flatten()
                    .and_then(|v| i32::try_from(v).ok()),
                precision: row
                    .try_get::<Option<u32>, _>("NUMERIC_PRECISION")
                    .ok()
                    .flatten()
                    .and_then(|v| i32::try_from(v).ok()),
                scale: row
                    .try_get::<Option<u32>, _>("NUMERIC_SCALE")
                    .ok()
                    .flatten()
                    .and_then(|v| i32::try_from(v).ok()),
                char_semantics: None,
                nullable,
                comment: row
                    .try_get::<Option<String>, _>("COLUMN_COMMENT")
                    .ok()
                    .flatten()
                    .filter(|v| !v.trim().is_empty()),
                default_value: row
                    .try_get::<Option<String>, _>("COLUMN_DEFAULT")
                    .ok()
                    .flatten(),
                identity: extra.contains("auto_increment"),
                identity_start: None,
                identity_increment: None,
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
    .fetch_all(&pool)
    .await
    .context("Failed to query MySQL primary keys")?;

    let primary_keys = pk_rows
        .into_iter()
        .map(|row| row.get::<String, _>("COLUMN_NAME"))
        .collect::<Vec<_>>();

    let indexes = fetch_indexes(&pool, schema_name, table_name).await?;
    let unique_constraints =
        fetch_unique_constraints(&pool, schema_name, table_name).await?;
    let foreign_keys = fetch_foreign_keys(&pool, schema_name, table_name).await?;
    let check_constraints =
        match fetch_check_constraints(&pool, schema_name, table_name).await {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    error = ?err,
                    schema = %schema_name,
                    table = %table_name,
                    "Failed to query MySQL check constraints; continuing with empty list"
                );
                Vec::new()
            }
        };
    let triggers = match fetch_triggers(&pool, schema_name, table_name).await {
        Ok(value) => value,
        Err(err) => {
            warn!(
                error = ?err,
                schema = %schema_name,
                table = %table_name,
                "Failed to query MySQL triggers; continuing with empty list"
            );
            Vec::new()
        }
    };

    pool.close().await;

    Ok(TableDetails {
        name: table_name.to_string(),
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

async fn fetch_indexes(pool: &sqlx::MySqlPool, schema: &str, table: &str) -> Result<Vec<Index>> {
    let rows = sqlx::query(
        "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE, SEQ_IN_INDEX \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
           AND INDEX_NAME <> 'PRIMARY' \
         ORDER BY INDEX_NAME, SEQ_IN_INDEX",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .context("Failed to query MySQL indexes")?;

    let mut grouped: HashMap<String, (bool, Vec<(u64, String)>)> = HashMap::new();
    for row in rows {
        let name = row.get::<String, _>("INDEX_NAME");
        let non_unique = row
            .try_get::<u8, _>("NON_UNIQUE")
            .map(|v| v == 1)
            .unwrap_or(true);
        let seq = row.try_get::<u64, _>("SEQ_IN_INDEX").unwrap_or(1);
        let column = row.get::<String, _>("COLUMN_NAME");

        let entry = grouped
            .entry(name)
            .or_insert_with(|| (!non_unique, Vec::new()));
        entry.0 = !non_unique;
        entry.1.push((seq, column));
    }

    let mut indexes = grouped
        .into_iter()
        .map(|(name, (unique, mut cols))| {
            cols.sort_by_key(|(seq, _)| *seq);
            Index {
                name,
                columns: cols.into_iter().map(|(_, col)| col).collect(),
                unique,
            }
        })
        .collect::<Vec<_>>();
    indexes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(indexes)
}

async fn fetch_unique_constraints(
    pool: &sqlx::MySqlPool,
    schema: &str,
    table: &str,
) -> Result<Vec<UniqueConstraint>> {
    let rows = sqlx::query(
        "SELECT tc.CONSTRAINT_NAME, kcu.COLUMN_NAME, kcu.ORDINAL_POSITION \
         FROM information_schema.TABLE_CONSTRAINTS tc \
         JOIN information_schema.KEY_COLUMN_USAGE kcu \
           ON tc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
          AND tc.TABLE_NAME = kcu.TABLE_NAME \
          AND tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
         WHERE tc.CONSTRAINT_SCHEMA = ? \
           AND tc.TABLE_NAME = ? \
           AND tc.CONSTRAINT_TYPE = 'UNIQUE' \
         ORDER BY tc.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .context("Failed to query MySQL unique constraints")?;

    let mut grouped: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    for row in rows {
        let name = row.get::<String, _>("CONSTRAINT_NAME");
        let col = row.get::<String, _>("COLUMN_NAME");
        let pos = row.try_get::<u64, _>("ORDINAL_POSITION").unwrap_or(1);
        grouped.entry(name).or_default().push((pos, col));
    }

    let mut uniques = grouped
        .into_iter()
        .map(|(name, mut cols)| {
            cols.sort_by_key(|(pos, _)| *pos);
            UniqueConstraint {
                name,
                columns: cols.into_iter().map(|(_, col)| col).collect(),
            }
        })
        .collect::<Vec<_>>();
    uniques.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(uniques)
}

pub async fn fetch_foreign_keys(
    pool: &sqlx::MySqlPool,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>> {
    let rows = sqlx::query(
        "SELECT rc.CONSTRAINT_NAME, \
                rc.DELETE_RULE, \
                rc.UPDATE_RULE, \
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
    .fetch_all(pool)
    .await
    .context("Failed to query MySQL foreign keys")?;

    #[derive(Default)]
    struct FkAgg {
        delete_rule: Option<String>,
        update_rule: Option<String>,
        referenced_table: Option<String>,
        columns: Vec<(u64, String)>,
        referenced_columns: Vec<(u64, String)>,
    }

    let mut grouped: HashMap<String, FkAgg> = HashMap::new();
    for row in rows {
        let name = row.get::<String, _>("CONSTRAINT_NAME");
        let seq = row.try_get::<u64, _>("ORDINAL_POSITION").unwrap_or(1);
        let column = row.get::<String, _>("COLUMN_NAME");
        let referenced_table = row.get::<String, _>("REFERENCED_TABLE_NAME");
        let referenced_column = row.get::<String, _>("REFERENCED_COLUMN_NAME");
        let delete_rule = row.try_get::<String, _>("DELETE_RULE").ok();
        let update_rule = row.try_get::<String, _>("UPDATE_RULE").ok();

        let agg = grouped.entry(name).or_default();
        if agg.delete_rule.is_none() {
            agg.delete_rule = delete_rule;
        }
        if agg.update_rule.is_none() {
            agg.update_rule = update_rule;
        }
        if agg.referenced_table.is_none() {
            agg.referenced_table = Some(referenced_table);
        }
        agg.columns.push((seq, column));
        agg.referenced_columns.push((seq, referenced_column));
    }

    let mut fks = grouped
        .into_iter()
        .map(|(name, mut agg)| {
            agg.columns.sort_by_key(|(pos, _)| *pos);
            agg.referenced_columns.sort_by_key(|(pos, _)| *pos);
            ForeignKey {
                name,
                columns: agg.columns.into_iter().map(|(_, col)| col).collect(),
                referenced_table: agg.referenced_table.unwrap_or_default(),
                referenced_columns: agg
                    .referenced_columns
                    .into_iter()
                    .map(|(_, col)| col)
                    .collect(),
                delete_rule: agg.delete_rule,
                update_rule: agg.update_rule,
            }
        })
        .collect::<Vec<_>>();
    fks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fks)
}

async fn fetch_check_constraints(
    pool: &sqlx::MySqlPool,
    schema: &str,
    table: &str,
) -> Result<Vec<CheckConstraint>> {
    let rows = sqlx::query(
        "SELECT tc.CONSTRAINT_NAME, cc.CHECK_CLAUSE \
         FROM information_schema.TABLE_CONSTRAINTS tc \
         JOIN information_schema.CHECK_CONSTRAINTS cc \
           ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA \
          AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME \
         WHERE tc.CONSTRAINT_SCHEMA = ? \
           AND tc.TABLE_NAME = ? \
           AND tc.CONSTRAINT_TYPE = 'CHECK' \
         ORDER BY tc.CONSTRAINT_NAME",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .context("Failed to query MySQL check constraints")?;

    Ok(rows
        .into_iter()
        .map(|row| CheckConstraint {
            name: row.get::<String, _>("CONSTRAINT_NAME"),
            condition: row.get::<String, _>("CHECK_CLAUSE"),
        })
        .collect())
}

async fn fetch_triggers(
    pool: &sqlx::MySqlPool,
    schema: &str,
    table: &str,
) -> Result<Vec<TriggerDefinition>> {
    let rows = sqlx::query(
        "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, ACTION_ORIENTATION, ACTION_STATEMENT \
         FROM information_schema.TRIGGERS \
         WHERE TRIGGER_SCHEMA = ? \
           AND EVENT_OBJECT_TABLE = ? \
         ORDER BY TRIGGER_NAME",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .context("Failed to query MySQL triggers")?;

    let mut grouped: HashMap<String, TriggerDefinition> = HashMap::new();
    for row in rows {
        let name = row.get::<String, _>("TRIGGER_NAME");
        let timing = row.get::<String, _>("ACTION_TIMING");
        let event = row.get::<String, _>("EVENT_MANIPULATION");
        let orientation = row
            .try_get::<String, _>("ACTION_ORIENTATION")
            .unwrap_or_else(|_| "ROW".to_string());
        let body = row.get::<String, _>("ACTION_STATEMENT");

        let entry = grouped
            .entry(name.clone())
            .or_insert_with(|| TriggerDefinition {
                name: name.clone(),
                table_name: table.to_string(),
                timing: timing.clone(),
                events: Vec::new(),
                each_row: orientation.eq_ignore_ascii_case("ROW"),
                body: body.clone(),
            });

        if !entry.events.iter().any(|existing| existing == &event) {
            entry.events.push(event);
        }
    }

    let mut triggers = grouped.into_values().collect::<Vec<_>>();
    triggers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(triggers)
}
