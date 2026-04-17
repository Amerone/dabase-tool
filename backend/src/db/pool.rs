use anyhow::{anyhow, Context, Result};
use odbc_api::{Connection, ConnectionOptions, Environment};
use parking_lot::Mutex;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::models::{ConnectionConfig, DbType};

static SHARED_ODBC_ENV: OnceLock<Arc<Environment>> = OnceLock::new();

fn shared_environment() -> Result<Arc<Environment>> {
    if let Some(env) = SHARED_ODBC_ENV.get() {
        return Ok(Arc::clone(env));
    }

    let env = Arc::new(Environment::new().context("Failed to initialize ODBC environment")?);

    if SHARED_ODBC_ENV.set(Arc::clone(&env)).is_err() {
        if let Some(existing) = SHARED_ODBC_ENV.get() {
            return Ok(Arc::clone(existing));
        }
        return Err(anyhow!("Failed to initialize shared ODBC environment"));
    }

    Ok(env)
}

/// Simplified connection pool that reuses Environment but creates connections on demand.
/// This is a significant improvement over creating a new Environment each time.
pub struct ConnectionPool {
    environment: Arc<Environment>,
    connection_string: String,
    schema: Option<String>,
    display_dsn: String,
    db_type: DbType,
    connection_count: Arc<Mutex<usize>>,
}

impl fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("dsn", &self.display_dsn)
            .field("schema", &self.schema)
            .field("db_type", &self.db_type)
            .field("active_connections", &self.connection_count.lock())
            .finish()
    }
}

impl ConnectionPool {
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        config
            .validate()
            .context("Invalid database connection configuration")?;

        if config.db_type == DbType::Mysql {
            return Err(anyhow!("MySQL should use native connector"));
        }
        if config.db_type == DbType::Shentong {
            return Err(anyhow!("ShenTong uses native ACI connector, not ODBC"));
        }

        let environment = shared_environment()?;
        let connection_string = config
            .odbc_connection_string()
            .context("Failed to build ODBC connection string")?;
        let schema = if config.schema.trim().is_empty() {
            None
        } else {
            Some(config.schema.clone())
        };

        let display_dsn = format!(
            "{:?} {}:{} as {}",
            config.db_type, config.host, config.port, config.username
        );

        Ok(Self {
            environment,
            connection_string,
            schema,
            display_dsn,
            db_type: config.db_type,
            connection_count: Arc::new(Mutex::new(0)),
        })
    }

    pub fn new_for_schema_discovery(config: ConnectionConfig) -> Result<Self> {
        config
            .validate_without_schema()
            .context("Invalid database connection configuration for schema discovery")?;

        if config.db_type == DbType::Mysql {
            return Err(anyhow!("MySQL should use native connector"));
        }
        if config.db_type == DbType::Shentong {
            return Err(anyhow!("ShenTong uses native ACI connector, not ODBC"));
        }

        let environment = shared_environment()?;
        let connection_string = config
            .odbc_connection_string()
            .context("Failed to build ODBC connection string")?;

        let display_dsn = format!(
            "{:?} {}:{} as {}",
            config.db_type, config.host, config.port, config.username
        );

        Ok(Self {
            environment,
            connection_string,
            schema: None,
            display_dsn,
            db_type: config.db_type,
            connection_count: Arc::new(Mutex::new(0)),
        })
    }

    pub fn test_connection(&self) -> Result<()> {
        let connection = self
            .get_connection()
            .context("Unable to open test connection")?;

        connection
            .execute("SELECT 1", ())
            .context("Connected but failed to execute health query")?;

        Ok(())
    }

    pub fn get_connection(&self) -> Result<Connection<'_>> {
        let mut connection = self
            .environment
            .connect_with_connection_string(&self.connection_string, ConnectionOptions::default())
            .with_context(|| format!("Failed to connect to database at {}", self.display_dsn))?;

        Self::apply_schema_static(&mut connection, &self.schema, &self.db_type)?;

        *self.connection_count.lock() += 1;
        tracing::debug!(
            "Created connection #{} to {}",
            *self.connection_count.lock(),
            self.display_dsn
        );

        Ok(connection)
    }

    fn apply_schema_static(
        connection: &mut Connection<'_>,
        schema: &Option<String>,
        db_type: &DbType,
    ) -> Result<()> {
        let Some(schema_name) = schema else {
            return Ok(());
        };

        if !is_valid_identifier(schema_name) {
            return Err(anyhow!(
                "Invalid schema name '{}': must contain only letters, digits, underscores, $, or #",
                schema_name
            ));
        }

        match db_type {
            DbType::Dm8 => {
                let statement = format!("SET SCHEMA \"{}\"", schema_name.replace('"', "\"\""));
                connection.execute(&statement, ()).with_context(|| {
                    format!("Connected but failed to set schema to '{}'", schema_name)
                })?;
            }
            DbType::Kingbase => {
                let statement = format!(
                    "SET search_path TO \"{}\"",
                    schema_name.replace('"', "\"\"")
                );
                connection.execute(&statement, ()).with_context(|| {
                    format!(
                        "Connected but failed to set search_path to '{}'",
                        schema_name
                    )
                })?;
            }
            DbType::Shentong => {
                // TODO(shentong): evaluate whether OSCAR requires session-level schema switching.
            }
            DbType::Mysql => {}
        }

        Ok(())
    }

    pub fn pool_stats(&self) -> PoolStats {
        PoolStats {
            total_connections_created: *self.connection_count.lock(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections_created: usize,
}

fn is_valid_identifier(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let first = trimmed.as_bytes()[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'#')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_identifier_accepts_valid_names() {
        assert!(is_valid_identifier("SYSDBA"));
        assert!(is_valid_identifier("_test"));
        assert!(is_valid_identifier("schema$1"));
        assert!(is_valid_identifier("test#123"));
    }

    #[test]
    fn is_valid_identifier_rejects_invalid_names() {
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("123abc"));
        assert!(!is_valid_identifier("test-name"));
        assert!(!is_valid_identifier("test name"));
    }
}
