use anyhow::{anyhow, Result};
use futures_util::{stream, StreamExt, TryStreamExt};
use parking_lot::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::db::{mysql, pg_native, pool::ConnectionPool, schema, shentong};
use crate::models::{ConnectionConfig, DbType, Table, TableDetails};

const TABLE_LIST_CACHE_TTL: Duration = Duration::from_secs(20);
const TABLE_DETAILS_CACHE_TTL: Duration = Duration::from_secs(90);
const TABLE_LIST_CACHE_MAX_ENTRIES: usize = 128;
const TABLE_DETAILS_CACHE_MAX_ENTRIES: usize = 256;
const TABLE_DETAILS_BATCH_CONCURRENCY: usize = 6;
const TABLE_DETAILS_BATCH_CHUNK_SIZE: usize = 100;

#[derive(Clone)]
struct CacheEntry<T> {
    value: T,
    expires_at: Instant,
    last_accessed: Instant,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            value,
            expires_at: now + ttl,
            last_accessed: now,
        }
    }
}

type TableListCacheMap = HashMap<String, CacheEntry<Vec<Table>>>;
type TableDetailsCacheMap = HashMap<String, CacheEntry<TableDetails>>;

static TABLE_LIST_CACHE: OnceLock<Mutex<TableListCacheMap>> = OnceLock::new();
static TABLE_DETAILS_CACHE: OnceLock<Mutex<TableDetailsCacheMap>> = OnceLock::new();

fn table_list_cache() -> &'static Mutex<TableListCacheMap> {
    TABLE_LIST_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn table_details_cache() -> &'static Mutex<TableDetailsCacheMap> {
    TABLE_DETAILS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn make_connection_key(config: &ConnectionConfig) -> String {
    let mut hasher = DefaultHasher::new();
    config.password.hash(&mut hasher);
    let password_hash = hasher.finish();

    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        config.db_type.filename_label(),
        config.host,
        config.port,
        config.username,
        password_hash,
        config.schema,
        config.database.as_deref().unwrap_or("")
    )
}

fn make_table_details_key(config: &ConnectionConfig, schema: &str, table: &str) -> String {
    let schema_key = schema.trim();
    let table_key = table.trim();
    format!(
        "{}|{}|{}",
        make_connection_key(config),
        schema_key,
        table_key
    )
}

fn normalize_requested_tables(tables: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut requested_tables = Vec::new();
    for name in tables {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_owned()) {
            requested_tables.push(trimmed.to_owned());
        }
    }
    requested_tables
}

fn prune_expired<T>(cache: &mut HashMap<String, CacheEntry<T>>) {
    let now = Instant::now();
    cache.retain(|_, entry| entry.expires_at > now);
}

fn enforce_cache_limit<T>(cache: &mut HashMap<String, CacheEntry<T>>, max_entries: usize) {
    while cache.len() > max_entries {
        let oldest_key = cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest_key {
            cache.remove(&key);
        } else {
            break;
        }
    }
}

fn get_cached_table_list(cache_key: &str) -> Option<Vec<Table>> {
    let mut cache = table_list_cache().lock();
    prune_expired(&mut cache);
    if let Some(entry) = cache.get_mut(cache_key) {
        entry.last_accessed = Instant::now();
        return Some(entry.value.clone());
    }
    None
}

fn set_cached_table_list(cache_key: String, tables: Vec<Table>) {
    let mut cache = table_list_cache().lock();
    prune_expired(&mut cache);
    cache.insert(cache_key, CacheEntry::new(tables, TABLE_LIST_CACHE_TTL));
    enforce_cache_limit(&mut cache, TABLE_LIST_CACHE_MAX_ENTRIES);
}

fn get_cached_table_details(cache_key: &str) -> Option<TableDetails> {
    let mut cache = table_details_cache().lock();
    prune_expired(&mut cache);
    if let Some(entry) = cache.get_mut(cache_key) {
        entry.last_accessed = Instant::now();
        return Some(entry.value.clone());
    }
    None
}

fn set_cached_table_details(cache_key: String, details: TableDetails) {
    let mut cache = table_details_cache().lock();
    prune_expired(&mut cache);
    cache.insert(cache_key, CacheEntry::new(details, TABLE_DETAILS_CACHE_TTL));
    enforce_cache_limit(&mut cache, TABLE_DETAILS_CACHE_MAX_ENTRIES);
}

fn merge_table_details_alias(
    config: &ConnectionConfig,
    schema: &str,
    table_name: &str,
    details: &TableDetails,
    merged: &mut HashMap<String, TableDetails>,
) {
    let table_key = table_name.trim();
    if table_key.is_empty() {
        return;
    }

    let cache_key = make_table_details_key(config, schema, table_key);
    set_cached_table_details(cache_key, details.clone());
    merged.insert(table_key.to_owned(), details.clone());
}

pub fn clear_metadata_caches() {
    table_list_cache().lock().clear();
    table_details_cache().lock().clear();
}

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
    let cache_key = make_connection_key(config);
    if let Some(cached) = get_cached_table_list(&cache_key) {
        tracing::debug!(
            db_type = %config.db_type.filename_label(),
            schema = %config.schema,
            "list_tables cache hit"
        );
        return Ok(cached);
    }

    let result = match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            run_blocking_task("dm8 list_tables", move || {
                let pool = ConnectionPool::new_for_schema_discovery(cfg.clone())?;
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
    };

    if let Ok(ref tables) = result {
        set_cached_table_list(cache_key, tables.clone());
    }

    result
}

pub async fn get_table_details(
    config: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let cache_key = make_table_details_key(config, schema, table);
    if let Some(cached) = get_cached_table_details(&cache_key) {
        tracing::debug!(
            db_type = %config.db_type.filename_label(),
            schema = %schema,
            table = %table,
            "get_table_details cache hit"
        );
        return Ok(cached);
    }

    let result = match config.db_type {
        DbType::Dm8 => {
            let cfg = config.clone();
            let schema_name = schema.to_string();
            let table_name = table.to_string();
            run_blocking_task("dm8 get_table_details", move || {
                let pool = ConnectionPool::new_for_schema_discovery(cfg.clone())?;
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
    };

    if let Ok(ref details) = result {
        set_cached_table_details(cache_key, details.clone());
    }

    result
}

pub async fn get_table_details_batch(
    config: &ConnectionConfig,
    schema: &str,
    tables: &[String],
) -> Result<Vec<TableDetails>> {
    if tables.is_empty() {
        return Ok(vec![]);
    }

    let requested_tables = normalize_requested_tables(tables);

    if requested_tables.is_empty() {
        return Ok(vec![]);
    }

    let mut merged: HashMap<String, TableDetails> = HashMap::new();
    let mut missing_tables = Vec::new();

    for table_name in &requested_tables {
        let cache_key = make_table_details_key(config, schema, table_name);
        if let Some(cached) = get_cached_table_details(&cache_key) {
            merged.insert(table_name.clone(), cached);
        } else {
            missing_tables.push(table_name.clone());
        }
    }

    if !missing_tables.is_empty() {
        match config.db_type {
            DbType::Dm8 => {
                let cfg = config.clone();
                let schema_name = schema.to_string();
                let table_names = missing_tables.clone();
                let requested_missing_tables = table_names.clone();
                let batch_details = run_blocking_task("dm8 get_table_details_batch", move || {
                    let pool = ConnectionPool::new_for_schema_discovery(cfg.clone())?;
                    let connection = pool.get_connection()?;
                    let mut merged_details = Vec::new();
                    for chunk in table_names.chunks(TABLE_DETAILS_BATCH_CHUNK_SIZE) {
                        let mut chunk_details =
                            schema::get_tables_details_batch(&connection, &schema_name, chunk)?;
                        merged_details.append(&mut chunk_details);
                    }
                    Ok(merged_details)
                })
                .await?;

                let details_by_canonical_name: HashMap<String, TableDetails> = batch_details
                    .into_iter()
                    .map(|details| (details.name.trim().to_uppercase(), details))
                    .collect();
                let mut resolved_details = Vec::with_capacity(requested_missing_tables.len());
                for requested_name in requested_missing_tables {
                    let canonical_name = requested_name.trim().to_uppercase();
                    let details = details_by_canonical_name
                        .get(&canonical_name)
                        .cloned()
                        .ok_or_else(|| {
                            anyhow!(
                                "Table details missing in DM8 batch response for '{}'",
                                requested_name
                            )
                        })?;
                    resolved_details.push((requested_name, details));
                }

                for (requested_name, details) in resolved_details {
                    merge_table_details_alias(
                        config,
                        schema,
                        &requested_name,
                        &details,
                        &mut merged,
                    );
                    if details.name.trim() != requested_name.trim() {
                        merge_table_details_alias(
                            config,
                            schema,
                            &details.name,
                            &details,
                            &mut merged,
                        );
                    }
                }
            }
            _ => {
                let cfg = config.clone();
                let schema_name = schema.to_owned();
                let details_list = stream::iter(missing_tables.into_iter())
                    .map(|table_name| {
                        let cfg = cfg.clone();
                        let schema_name = schema_name.clone();
                        async move {
                            get_table_details(&cfg, &schema_name, &table_name)
                                .await
                                .map(|details| (table_name, details))
                        }
                    })
                    .buffer_unordered(TABLE_DETAILS_BATCH_CONCURRENCY)
                    .try_collect::<Vec<_>>()
                    .await?;
                for (requested_name, details) in details_list {
                    merge_table_details_alias(
                        config,
                        schema,
                        &requested_name,
                        &details,
                        &mut merged,
                    );
                    if details.name.trim() != requested_name.trim() {
                        merge_table_details_alias(
                            config,
                            schema,
                            &details.name,
                            &details,
                            &mut merged,
                        );
                    }
                }
            }
        }
    }

    let mut ordered = Vec::with_capacity(requested_tables.len());
    for table_name in requested_tables {
        if let Some(details) = merged.get(&table_name) {
            ordered.push(details.clone());
            continue;
        }
        return Err(anyhow!(
            "Table details missing in batch response for '{}'",
            table_name
        ));
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::normalize_requested_tables;

    #[test]
    fn normalize_requested_tables_deduplicates_exact_values_and_preserves_order() {
        let tables = vec![
            " USERS ".to_string(),
            "USERS".to_string(),
            "users".to_string(),
            "orders".to_string(),
            "ORDERS  ".to_string(),
            "".to_string(),
            "   ".to_string(),
            "products".to_string(),
        ];

        let normalized = normalize_requested_tables(&tables);
        assert_eq!(
            normalized,
            vec![
                "USERS".to_string(),
                "users".to_string(),
                "orders".to_string(),
                "ORDERS".to_string(),
                "products".to_string(),
            ]
        );
    }
}
