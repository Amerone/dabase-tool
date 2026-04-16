use std::{fs, path::PathBuf};

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, ensure, Context, Result};
use base64::Engine;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ConfigSource, ConnectionConfig, DbType};

#[derive(Debug, Clone)]
pub struct StoredConnection {
    pub config: ConnectionConfig,
    pub source: ConfigSource,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NamedStoredConnection {
    pub id: i64,
    pub name: String,
    pub config: ConnectionConfig,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    db_path: PathBuf,
    encryption_key: [u8; 32],
}

// --------------- encryption helpers ---------------

const ENC_PREFIX: &str = "enc:";

fn db_type_as_str(db_type: &DbType) -> &'static str {
    match db_type {
        DbType::Dm8 => "dm8",
        DbType::Mysql => "mysql",
        DbType::Kingbase => "kingbase",
        DbType::Shentong => "shentong",
    }
}

fn db_type_from_str(value: &str) -> Result<DbType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "dm8" => Ok(DbType::Dm8),
        "mysql" => Ok(DbType::Mysql),
        "kingbase" => Ok(DbType::Kingbase),
        "shentong" => Ok(DbType::Shentong),
        other => Err(anyhow!(
            "Unsupported db_type value '{}' in config store",
            other
        )),
    }
}

fn default_connection_name(db_type: &DbType) -> String {
    format!("default-{}", db_type_as_str(db_type))
}

fn load_or_create_key(key_path: &std::path::Path) -> Result<[u8; 32]> {
    if key_path.exists() {
        let bytes = fs::read(key_path)
            .with_context(|| format!("Failed to read encryption key at {:?}", key_path))?;
        ensure!(bytes.len() == 32, "Encryption key must be 32 bytes");
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(key)
    } else {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        fs::write(key_path, key)
            .with_context(|| format!("Failed to write encryption key to {:?}", key_path))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(key_path, fs::Permissions::from_mode(0o600)).ok();
        }
        Ok(key)
    }
}

fn encrypt_value(plaintext: &str, key: &[u8; 32]) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| anyhow!("AES-GCM encryption failed"))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        ENC_PREFIX,
        base64::engine::general_purpose::STANDARD.encode(&combined)
    ))
}

fn decrypt_value(stored: &str, key: &[u8; 32]) -> Result<String> {
    if !stored.starts_with(ENC_PREFIX) {
        // Legacy plaintext — return as-is for backward compatibility.
        return Ok(stored.to_string());
    }
    let encoded = &stored[ENC_PREFIX.len()..];
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("Invalid base64 in encrypted password")?;
    ensure!(combined.len() > 12, "Encrypted value too short");
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow!("AES-GCM decryption failed (wrong key or corrupt data)"))?;
    String::from_utf8(plaintext).context("Decrypted value is not valid UTF-8")
}

// --------------- ConfigStore ---------------

impl ConfigStore {
    pub fn new_with_path(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }

        let key_path = db_path
            .parent()
            .ok_or_else(|| anyhow!("DB path has no parent directory"))?
            .join(".key");
        let encryption_key = load_or_create_key(&key_path)?;

        let store = Self {
            db_path,
            encryption_key,
        };
        store.init_db()?;
        Ok(store)
    }

    pub fn ensure_default_path() -> Result<Self> {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;
        let db_path = home_dir.join(".amarone").join("config.db");
        Self::new_with_path(db_path)
    }

    pub fn get_default(&self) -> Result<Option<StoredConnection>> {
        self.get_default_for(None)
    }

    pub fn get_default_for(&self, db_type: Option<DbType>) -> Result<Option<StoredConnection>> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let row = if let Some(db_type) = db_type {
            let connection_name = default_connection_name(&db_type);
            let mut stmt = conn.prepare(
                "SELECT db_type, host, port, username, password, schema, export_schema, updated_at, database \
                 FROM connections \
                 WHERE name = ?1 \
                 LIMIT 1",
            )?;
            stmt.query_row([connection_name], |row| {
                let db_type_raw: String = row.get(0)?;
                let port: i64 = row.get(2)?;
                let port = u16::try_from(port).unwrap_or_default();
                Ok((
                    db_type_raw,
                    row.get::<_, String>(1)?,
                    port,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .optional()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT db_type, host, port, username, password, schema, export_schema, updated_at, database \
                 FROM connections \
                 WHERE name LIKE 'default-%' \
                 ORDER BY updated_at DESC LIMIT 1",
            )?;
            stmt.query_row([], |row| {
                let db_type_raw: String = row.get(0)?;
                let port: i64 = row.get(2)?;
                let port = u16::try_from(port).unwrap_or_default();
                Ok((
                    db_type_raw,
                    row.get::<_, String>(1)?,
                    port,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .optional()?
        };

        match row {
            Some((
                db_type_raw,
                host,
                port,
                username,
                stored_password,
                schema,
                export_schema,
                updated_at,
                database,
            )) => {
                let db_type = db_type_from_str(&db_type_raw)?;
                let password = decrypt_value(&stored_password, &self.encryption_key)?;
                Ok(Some(StoredConnection {
                    config: ConnectionConfig {
                        db_type,
                        host,
                        port,
                        username,
                        password,
                        schema,
                        export_schema,
                        database,
                    },
                    source: ConfigSource::Sqlite,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    pub fn upsert_default(&self, config: &ConnectionConfig) -> Result<StoredConnection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let updated_at = Utc::now().to_rfc3339();
        let encrypted_password = encrypt_value(&config.password, &self.encryption_key)?;
        let connection_name = default_connection_name(&config.db_type);
        let db_type = db_type_as_str(&config.db_type);

        conn.execute(
            "INSERT INTO connections (name, db_type, host, port, username, password, schema, export_schema, database, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(name) DO UPDATE SET \
             db_type=excluded.db_type, host=excluded.host, port=excluded.port, \
             username=excluded.username, password=excluded.password, schema=excluded.schema, \
             export_schema=excluded.export_schema, database=excluded.database, updated_at=excluded.updated_at",
            params![
                &connection_name,
                db_type,
                &config.host,
                config.port as i64,
                &config.username,
                &encrypted_password,
                &config.schema,
                &config.export_schema,
                &config.database,
                &updated_at
            ],
        )?;

        Ok(StoredConnection {
            config: config.clone(),
            source: ConfigSource::Sqlite,
            updated_at: Some(updated_at),
        })
    }

    pub fn list_connections(&self) -> Result<Vec<NamedStoredConnection>> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let mut stmt = conn.prepare(
            "SELECT id, name, db_type, host, port, username, password, schema, export_schema, updated_at, database \
             FROM connections ORDER BY updated_at DESC, id DESC",
        )?;

        let mut rows = stmt.query([])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(self.named_row_to_connection(row)?);
        }

        Ok(result)
    }

    pub fn upsert_named(
        &self,
        name: &str,
        config: &ConnectionConfig,
    ) -> Result<NamedStoredConnection> {
        let normalized_name = name.trim();
        ensure!(
            !normalized_name.is_empty(),
            "Connection name cannot be empty"
        );

        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let updated_at = Utc::now().to_rfc3339();
        let encrypted_password = encrypt_value(&config.password, &self.encryption_key)?;
        let db_type = db_type_as_str(&config.db_type);

        conn.execute(
            "INSERT INTO connections (name, db_type, host, port, username, password, schema, export_schema, database, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(name) DO UPDATE SET \
             db_type=excluded.db_type, host=excluded.host, port=excluded.port, \
             username=excluded.username, password=excluded.password, schema=excluded.schema, \
             export_schema=excluded.export_schema, database=excluded.database, updated_at=excluded.updated_at",
            params![
                normalized_name,
                db_type,
                &config.host,
                config.port as i64,
                &config.username,
                &encrypted_password,
                &config.schema,
                &config.export_schema,
                &config.database,
                &updated_at
            ],
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, name, db_type, host, port, username, password, schema, export_schema, updated_at, database \
             FROM connections WHERE name = ?1 LIMIT 1",
        )?;
        let row = stmt
            .query_row([normalized_name], |row| self.named_row_to_connection(row))
            .optional()?;

        row.ok_or_else(|| anyhow!("Saved connection '{}' was not found", normalized_name))
    }

    pub fn delete_connection_by_id(&self, id: i64) -> Result<bool> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", self.db_path))?;

        let affected = conn.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
        Ok(affected > 0)
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
                export_schema TEXT,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        ensure_export_schema_column(&conn)?;
        ensure_database_column(&conn)?;

        Ok(())
    }

    fn named_row_to_connection(
        &self,
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<NamedStoredConnection> {
        let db_type_raw: String = row.get(2)?;
        let db_type = db_type_from_str(&db_type_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?;
        let port_raw: i64 = row.get(4)?;
        let port = u16::try_from(port_raw).unwrap_or_default();
        let stored_password: String = row.get(6)?;
        let password = decrypt_value(&stored_password, &self.encryption_key).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?;

        Ok(NamedStoredConnection {
            id: row.get(0)?,
            name: row.get(1)?,
            config: ConnectionConfig {
                db_type,
                host: row.get(3)?,
                port,
                username: row.get(5)?,
                password,
                schema: row.get(7)?,
                export_schema: row.get(8)?,
                database: row.get(10)?,
            },
            updated_at: row.get(9)?,
        })
    }
}

fn ensure_export_schema_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(connections)")?;
    let mut rows = stmt.query([])?;
    let mut has_column = false;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "export_schema" {
            has_column = true;
            break;
        }
    }

    if !has_column {
        conn.execute("ALTER TABLE connections ADD COLUMN export_schema TEXT", [])?;
    }

    Ok(())
}

fn ensure_database_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(connections)")?;
    let mut rows = stmt.query([])?;
    let mut has_column = false;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "database" {
            has_column = true;
            break;
        }
    }

    if !has_column {
        conn.execute("ALTER TABLE connections ADD COLUMN database TEXT", [])?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};
    use tempfile::TempDir;

    fn sample_config() -> ConnectionConfig {
        ConnectionConfig {
            db_type: DbType::Dm8,
            host: "localhost".into(),
            port: 5236,
            username: "SYSDBA".into(),
            password: "SYSDBA".into(),
            schema: "SYSDBA".into(),
            export_schema: Some("APP".into()),
            database: None,
        }
    }

    #[test]
    fn get_default_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let result = store.get_default().unwrap();
        assert!(
            result.is_none(),
            "Expected no record when database is empty"
        );
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
        assert_eq!(saved.config.export_schema.as_deref(), Some("APP"));
        assert!(saved.updated_at.is_some());

        let fetched = store.get_default().unwrap().unwrap();
        assert_eq!(fetched.config.db_type, DbType::Dm8);
        assert_eq!(fetched.config.username, "SYSDBA");
        assert_eq!(fetched.config.password, "SYSDBA");
        assert_eq!(fetched.source, ConfigSource::Sqlite);
        assert_eq!(fetched.config.schema, "SYSDBA");
        assert_eq!(fetched.config.export_schema.as_deref(), Some("APP"));
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

        assert_ne!(
            first_ts, second.updated_at,
            "timestamp should update on overwrite"
        );

        let fetched = store.get_default().unwrap().unwrap();
        assert_eq!(fetched.config.host, "127.0.0.1");
    }

    #[test]
    fn get_default_for_respects_db_type_filter() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let mut dm8 = sample_config();
        dm8.db_type = DbType::Dm8;
        dm8.host = "dm8-host".to_string();
        store.upsert_default(&dm8).unwrap();

        let mut mysql = sample_config();
        mysql.db_type = DbType::Mysql;
        mysql.host = "mysql-host".to_string();
        store.upsert_default(&mysql).unwrap();

        let latest = store.get_default().unwrap().unwrap();
        assert_eq!(latest.config.db_type, DbType::Mysql);

        let dm8_default = store.get_default_for(Some(DbType::Dm8)).unwrap().unwrap();
        assert_eq!(dm8_default.config.host, "dm8-host");
        assert_eq!(dm8_default.config.db_type, DbType::Dm8);

        let mysql_default = store.get_default_for(Some(DbType::Mysql)).unwrap().unwrap();
        assert_eq!(mysql_default.config.host, "mysql-host");
        assert_eq!(mysql_default.config.db_type, DbType::Mysql);
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = [42u8; 32];
        let original = "my_secret_password";
        let encrypted = encrypt_value(original, &key).unwrap();
        assert!(encrypted.starts_with("enc:"));
        assert_ne!(encrypted, original);
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn decrypt_handles_legacy_plaintext() {
        let key = [42u8; 32];
        let plaintext = "old_plain_password";
        let result = decrypt_value(plaintext, &key).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn password_stored_encrypted_in_sqlite() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path.clone()).unwrap();

        let config = sample_config();
        store.upsert_default(&config).unwrap();

        // Read raw password from SQLite — should be encrypted, not plaintext.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT password FROM connections WHERE name = 'default-dm8'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            raw.starts_with("enc:"),
            "password should be encrypted in DB"
        );
        assert!(!raw.contains("SYSDBA"));
    }

    #[test]
    fn upsert_named_and_list_connections() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let mut config = sample_config();
        config.db_type = DbType::Mysql;
        config.schema = "demo".to_string();
        let saved = store.upsert_named("mysql-dev", &config).unwrap();

        assert!(saved.id > 0);
        assert_eq!(saved.name, "mysql-dev");
        assert_eq!(saved.config.db_type, DbType::Mysql);

        let list = store.list_connections().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "mysql-dev");
        assert_eq!(list[0].config.schema, "demo");
    }

    #[test]
    fn delete_connection_by_id_removes_record() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("config.db");
        let store = ConfigStore::new_with_path(db_path).unwrap();

        let config = sample_config();
        let saved = store.upsert_named("dm8-main", &config).unwrap();

        let deleted = store.delete_connection_by_id(saved.id).unwrap();
        assert!(deleted);
        assert!(store.list_connections().unwrap().is_empty());
    }
}
