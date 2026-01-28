use std::{fs, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ConfigSource, ConnectionConfig};

#[derive(Debug, Clone)]
pub struct StoredConnection {
    pub config: ConnectionConfig,
    pub source: ConfigSource,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    db_path: PathBuf,
}

impl ConfigStore {
    pub fn new_with_path(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }

        let store = Self { db_path };
        store.init_db()?;
        Ok(store)
    }

    pub fn ensure_default_path() -> Result<Self> {
        let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;
        let db_path = home_dir.join(".amarone").join("config.db");
        Self::new_with_path(db_path)
    }

    pub fn get_default(&self) -> Result<Option<StoredConnection>> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let mut stmt = conn.prepare(
            "SELECT db_type, host, port, username, password, schema, updated_at \
             FROM connections WHERE name = ?1 LIMIT 1",
        )?;

        let row = stmt
            .query_row(params!["default-dm8"], |row| {
                let port: i64 = row.get(2)?;
                let port = u16::try_from(port).unwrap_or_default();
                Ok(StoredConnection {
                    config: ConnectionConfig {
                        host: row.get(1)?,
                        port,
                        username: row.get(3)?,
                        password: row.get(4)?,
                        schema: row.get(5)?,
                    },
                    source: ConfigSource::Sqlite,
                    updated_at: row.get(6)?,
                })
            })
            .optional()?;

        Ok(row)
    }

    pub fn upsert_default(&self, config: &ConnectionConfig) -> Result<StoredConnection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let updated_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO connections (name, db_type, host, port, username, password, schema, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(name) DO UPDATE SET \
             db_type=excluded.db_type, host=excluded.host, port=excluded.port, \
             username=excluded.username, password=excluded.password, schema=excluded.schema, \
             updated_at=excluded.updated_at",
            params![
                "default-dm8",
                "dm8",
                &config.host,
                config.port as i64,
                &config.username,
                &config.password,
                &config.schema,
                &updated_at
            ],
        )?;

        Ok(StoredConnection {
            config: config.clone(),
            source: ConfigSource::Sqlite,
            updated_at: Some(updated_at),
        })
    }

    fn init_db(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                db_type TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                username TEXT NOT NULL,
                password TEXT NOT NULL,
                schema TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};
    use tempfile::TempDir;

    fn sample_config() -> ConnectionConfig {
        ConnectionConfig {
            host: "localhost".into(),
            port: 5236,
            username: "SYSDBA".into(),
            password: "SYSDBA".into(),
            schema: "SYSDBA".into(),
        }
    }

    #[test]
    fn get_default_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let result = store.get_default().unwrap();
        assert!(result.is_none(), "Expected no record when database is empty");
    }

    #[test]
    fn upsert_and_get_default_round_trip() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let config = sample_config();
        let saved = store.upsert_default(&config).unwrap();
        assert_eq!(saved.config.host, "localhost");
        assert_eq!(saved.source, ConfigSource::Sqlite);
        assert!(saved.updated_at.is_some());

        let fetched = store.get_default().unwrap().unwrap();
        assert_eq!(fetched.config.username, "SYSDBA");
        assert_eq!(fetched.source, ConfigSource::Sqlite);
        assert_eq!(fetched.config.schema, "SYSDBA");
        assert!(fetched.updated_at.is_some());
    }

    #[test]
    fn upsert_updates_timestamp_on_overwrite() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let mut config = sample_config();
        let first = store.upsert_default(&config).unwrap();
        let first_ts = first.updated_at.clone();
        assert!(first_ts.is_some());

        thread::sleep(Duration::from_millis(5));
        config.host = "127.0.0.1".into();
        let second = store.upsert_default(&config).unwrap();

        assert_ne!(first_ts, second.updated_at, "timestamp should update on overwrite");

        let fetched = store.get_default().unwrap().unwrap();
        assert_eq!(fetched.config.host, "127.0.0.1");
    }
}
