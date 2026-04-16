use anyhow::{anyhow, Result};

use crate::db::{mysql, pg_native, pool::ConnectionPool, schema, shentong};
use crate::models::{ConnectionConfig, DbType, Table, TableDetails};

async fn run_blocking_task<T, F>(task_name: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| anyhow!("{} failed to join: {}", task_name, e))?
}

pub async fn test_connection(config: &ConnectionConfig) -> Result<()> {
    match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            run_blocking_task("dm8 test_connection", move || {
                let pool = ConnectionPool::new(cfg)?;
                pool.test_connection()
            })
            .await
        }
        DbType::Mysql => mysql::test_connection(config).await,
        DbType::Kingbase => pg_native::test_connection(config).await,
        DbType::Shentong => {
            let cfg = config.clone();
            run_blocking_task("shentong test_connection", move || {
                shentong::test_connection(&cfg)
            })
            .await
        }
    }
}

pub async fn list_schemas(config: &ConnectionConfig) -> Result<Vec<String>> {
    match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            run_blocking_task("dm8 list_schemas", move || {
                let pool = ConnectionPool::new_for_schema_discovery(cfg)?;
                let connection = pool.get_connection()?;
                schema::get_schemas(&connection)
            })
            .await
        }
        DbType::Mysql => mysql::get_schemas(config).await,
        DbType::Kingbase => pg_native::get_schemas(config).await,
        DbType::Shentong => {
            let cfg = config.clone();
            run_blocking_task("shentong list_schemas", move || shentong::get_schemas(&cfg)).await
        }
    }
}

pub async fn list_tables(config: &ConnectionConfig) -> Result<Vec<Table>> {
    match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            run_blocking_task("dm8 list_tables", move || {
                let pool = ConnectionPool::new(cfg.clone())?;
                let connection = pool.get_connection()?;
                schema::get_tables(&connection, &cfg.schema)
            })
            .await
        }
        DbType::Mysql => mysql::get_tables(config).await,
        DbType::Kingbase => pg_native::get_tables(config).await,
        DbType::Shentong => {
            let cfg = config.clone();
            run_blocking_task("shentong list_tables", move || shentong::get_tables(&cfg)).await
        }
    }
}

pub async fn get_table_details(
    config: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            let schema_name = schema.to_string();
            let table_name = table.to_string();
            run_blocking_task("dm8 get_table_details", move || {
                let pool = ConnectionPool::new(cfg.clone())?;
                let connection = pool.get_connection()?;
                schema::get_table_details(&connection, &schema_name, &table_name)
            })
            .await
        }
        DbType::Mysql => mysql::get_table_details(config, schema, table).await,
        DbType::Kingbase => pg_native::get_table_details(config, schema, table).await,
        DbType::Shentong => {
            let cfg = config.clone();
            let schema_name = schema.to_string();
            let table_name = table.to_string();
            run_blocking_task("shentong get_table_details", move || {
                shentong::get_table_details(&cfg, &schema_name, &table_name)
            })
            .await
        }
    }
}
