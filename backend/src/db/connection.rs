use anyhow::{anyhow, ensure, Result};

use crate::db::odbc_register;
use crate::models::{ConnectionConfig, DbType};

#[allow(dead_code)]
fn is_valid_identifier(name: &str) -> bool {
    let trimmed = name.trim();

    // Safely get the first byte
    let first = match trimmed.as_bytes().first() {
        Some(&b) => b,
        None => return false,
    };

    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'#')
}

fn wrap_driver(driver: &str) -> String {
    let trimmed = driver.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return trimmed.to_string();
    }
    format!("{{{}}}", trimmed)
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Render an ODBC attribute value.
///
/// Most values can stay unwrapped (`UID=SYSDBA`). Only wrap with braces when
/// the value contains separators or edge whitespace; this keeps DM8 drivers
/// that are strict about UID/PWD parsing compatible.
fn render_odbc_attr_value(value: &str) -> String {
    let starts_or_ends_with_ws = value
        .chars()
        .next()
        .map(|c| c.is_whitespace())
        .unwrap_or(false)
        || value
            .chars()
            .next_back()
            .map(|c| c.is_whitespace())
            .unwrap_or(false);

    let needs_wrap = value.contains(';') || value.contains('}') || starts_or_ends_with_ws;

    if needs_wrap {
        format!("{{{}}}", value.replace('}', "}}"))
    } else {
        value.to_string()
    }
}

/// Find the first existing file path from a list of candidates.
///
/// # Arguments
/// * `candidates` - A slice of file path strings to check
///
/// # Returns
/// * `Some(String)` - The first path that exists
/// * `None` - If no paths exist or the list is empty
///
/// # Examples
/// ```no_run
/// let paths = ["file1.txt", "file2.txt"];
/// // first_existing_path is a private helper; usage shown for documentation only
/// ```
fn first_existing_path(candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        let path = std::path::Path::new(candidate);
        path.exists().then(|| candidate.to_string())
    })
}

impl ConnectionConfig {
    pub fn validate_without_schema(&self) -> Result<()> {
        ensure!(!self.host.trim().is_empty(), "Database host is required");
        ensure!(self.port > 0, "Database port must be greater than zero");
        ensure!(
            !self.username.trim().is_empty(),
            "Database username is required"
        );
        ensure!(!self.password.is_empty(), "Database password is required");
        Ok(())
    }

    fn dm8_driver_value() -> String {
        #[cfg(windows)]
        {
            let configured = env_nonempty("DM8_ODBC_DRIVER").unwrap_or_else(|| {
                if odbc_register::is_registered_in_hklm(odbc_register::DM8_DRIVER_NAME) {
                    odbc_register::DM8_DRIVER_NAME.to_string()
                } else if odbc_register::is_registered_in_hklm(
                    odbc_register::DM8_SYSTEM_DRIVER_NAME,
                ) {
                    odbc_register::DM8_SYSTEM_DRIVER_NAME.to_string()
                } else {
                    odbc_register::DM8_DRIVER_NAME.to_string()
                }
            });
            wrap_driver(&configured)
        }

        #[cfg(not(windows))]
        {
            if let Some(path) = env_nonempty("DM8_DRIVER_PATH") {
                tracing::debug!("DM8 driver from DM8_DRIVER_PATH: {}", path);
                return wrap_driver(&path);
            }

            let candidates = ["drivers/dm8/libdodbc.so", "../drivers/dm8/libdodbc.so"];
            if let Some(path) = first_existing_path(&candidates) {
                tracing::debug!("DM8 driver from bundled path: {}", path);
                return wrap_driver(&path);
            }

            tracing::warn!("DM8_DRIVER_PATH not set and no bundled driver found; falling back to registered name");
            wrap_driver("DM8 ODBC DRIVER")
        }
    }

    fn kingbase_driver_value() -> String {
        if let Some(path) = env_nonempty("KINGBASE_ODBC_DRIVER_PATH") {
            tracing::debug!("Kingbase driver from KINGBASE_ODBC_DRIVER_PATH: {}", path);
            return wrap_driver(&path);
        }

        #[cfg(not(windows))]
        {
            let candidates = [
                "drivers/kingbase/libkdbodbcw.so",
                "drivers/kingbase/libkdbodbc.so",
                "drivers/kingbase/X64_Linux/odbc/kdbodbcw.so",
                "../drivers/kingbase/libkdbodbcw.so",
                "../drivers/kingbase/libkdbodbc.so",
                "../drivers/kingbase/X64_Linux/odbc/kdbodbcw.so",
            ];
            if let Some(path) = first_existing_path(&candidates) {
                tracing::debug!("Kingbase driver from bundled path: {}", path);
                return wrap_driver(&path);
            }
        }

        #[cfg(windows)]
        {
            // Try PostgreSQL ODBC driver first (KingBase is PostgreSQL-compatible).
            // IMPORTANT: Use the absolute file path, NOT the registered driver name.
            // Windows ODBC Driver Manager only resolves DRIVER={name} via HKLM, not HKCU.
            // Our drivers are registered under HKCU (no admin), so using a name causes IM002.
            // Using an absolute path bypasses the registry lookup entirely.
            // NOTE: strip \\?\ prefix from canonicalize() — the legacy ODBC DM (Win32) does
            // not support extended-length path syntax and returns IM002 when it sees it.
            if let Some(path) = first_existing_path(odbc_register::POSTGRESQL_DRIVER_CANDIDATES) {
                let abs = std::path::Path::new(&path)
                    .canonicalize()
                    .map(|p| {
                        let s = p.to_string_lossy().into_owned();
                        // Strip Windows extended-length path prefix \\?\ if present
                        s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned()
                    })
                    .unwrap_or(path);
                tracing::info!(
                    "Using PostgreSQL ODBC driver for KingBase connection (absolute path): {}",
                    abs
                );
                return wrap_driver(&abs);
            }

            // Fallback to KingBase native driver (also by path for the same reason)
            if let Some(path) = first_existing_path(odbc_register::KINGBASE_DRIVER_CANDIDATES) {
                let abs = std::path::Path::new(&path)
                    .canonicalize()
                    .map(|p| {
                        let s = p.to_string_lossy().into_owned();
                        s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned()
                    })
                    .unwrap_or(path);
                tracing::debug!("Kingbase native driver from bundled path: {}", abs);
                return wrap_driver(&abs);
            }
        }

        let name = env_nonempty("KINGBASE_ODBC_DRIVER")
            .unwrap_or_else(|| odbc_register::KINGBASE_DRIVER_NAME.to_string());
        wrap_driver(&name)
    }

    fn shentong_driver_value() -> String {
        if let Some(path) = env_nonempty("SHENTONG_ODBC_DRIVER_PATH") {
            tracing::debug!("Shentong driver from SHENTONG_ODBC_DRIVER_PATH: {}", path);
            return wrap_driver(&path);
        }

        #[cfg(not(windows))]
        {
            let candidates = [
                "drivers/shentong/liboscarodbcw.so",
                "drivers/shentong/liboscarodbc.so",
                "../drivers/shentong/liboscarodbcw.so",
                "../drivers/shentong/liboscarodbc.so",
            ];
            if let Some(path) = first_existing_path(&candidates) {
                tracing::debug!("Shentong driver from bundled path: {}", path);
                return wrap_driver(&path);
            }
        }

        let name =
            env_nonempty("SHENTONG_ODBC_DRIVER").unwrap_or_else(|| "OSCAR ODBC DRIVER".to_string());
        wrap_driver(&name)
    }

    fn odbc_driver_value(&self) -> Result<String> {
        match self.db_type {
            DbType::Dm8 => Ok(Self::dm8_driver_value()),
            DbType::Kingbase => Ok(Self::kingbase_driver_value()),
            DbType::Shentong => Ok(Self::shentong_driver_value()),
            DbType::Mysql => Err(anyhow!("MySQL uses native driver, not ODBC")),
        }
    }

    pub fn odbc_connection_string(&self) -> Result<String> {
        let driver = self.odbc_driver_value()?;
        let server = render_odbc_attr_value(self.host.trim());
        let uid = render_odbc_attr_value(self.username.trim());
        let pwd = render_odbc_attr_value(&self.password);

        let cs = match self.db_type {
            DbType::Dm8 => {
                // SCHEMA is NOT a valid DM8 ODBC connection string parameter — schema selection
                // is done via `SET SCHEMA` statement after connection (see apply_schema_static).
                // DM8 follows ODBC spec: brace-wrapped values handle special chars (e.g. `;` in PWD).
                format!(
                    "DRIVER={};SERVER={};TCP_PORT={};PORT={};UID={};PWD={};CHARSET=1",
                    driver, server, self.port, self.port, uid, pwd
                )
            }
            DbType::Kingbase | DbType::Shentong => {
                // Prefer explicit database field; fall back to schema for DATABASE param
                let db_value = self
                    .database
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(self.schema.trim());
                if db_value.is_empty() {
                    format!(
                        "DRIVER={};SERVER={};PORT={};UID={};PWD={}",
                        driver, server, self.port, uid, pwd
                    )
                } else {
                    let database = render_odbc_attr_value(db_value);
                    format!(
                        "DRIVER={};SERVER={};PORT={};UID={};PWD={};DATABASE={}",
                        driver, server, self.port, uid, pwd, database
                    )
                }
            }
            DbType::Mysql => return Err(anyhow!("MySQL uses native driver, not ODBC")),
        };

        tracing::debug!(
            driver = %driver,
            server = %self.host,
            port = %self.port,
            username = %self.username,
            database = %self.schema,
            password_length = %self.password.len(),
            "ODBC connection parameters"
        );

        Ok(cs)
    }
}

#[cfg(test)]
mod tests {
    use super::{render_odbc_attr_value, ConnectionConfig};
    use crate::models::DbType;

    #[test]
    fn render_odbc_attr_value_wraps_and_escapes_brace() {
        assert_eq!(render_odbc_attr_value("ab};c"), "{ab}};c}");
    }

    #[test]
    fn render_odbc_attr_value_keeps_simple_value_unwrapped() {
        assert_eq!(render_odbc_attr_value("SYSDBA"), "SYSDBA");
    }

    #[test]
    fn connection_string_escapes_semicolon_in_password() {
        let config = ConnectionConfig {
            db_type: DbType::Dm8,
            host: "127.0.0.1".to_string(),
            port: 5236,
            username: "SYSDBA".to_string(),
            password: "pa;ss".to_string(),
            schema: "SYSDBA".to_string(),
            export_schema: None,
            database: None,
        };

        let cs = config
            .odbc_connection_string()
            .expect("connection string should be rendered");
        assert!(
            cs.contains("PWD={pa;ss}"),
            "unexpected connection string: {cs}"
        );
    }

    #[test]
    fn dm8_connection_string_includes_tcp_port() {
        let config = ConnectionConfig {
            db_type: DbType::Dm8,
            host: "192.168.0.122".to_string(),
            port: 5237,
            username: "PLATFORM".to_string(),
            password: "123456789".to_string(),
            schema: "platform".to_string(),
            export_schema: None,
            database: None,
        };

        let cs = config
            .odbc_connection_string()
            .expect("connection string should be rendered");
        assert!(
            cs.contains("SERVER=192.168.0.122;TCP_PORT=5237"),
            "unexpected connection string: {cs}"
        );
    }
}
