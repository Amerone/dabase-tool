use anyhow::{ensure, Context, Result};
use odbc_api::{Connection, ConnectionOptions, Environment};
use std::fmt;

use crate::db::odbc_register;
use crate::models::ConnectionConfig;

/// Returns true if `name` is a valid DM8 identifier (letters, digits, _, $, #).
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

impl ConnectionConfig {
    /// Returns the ODBC driver value; prefers an explicit path from `DM8_DRIVER_PATH`.
    fn driver_value() -> String {
        // On Windows the Driver Manager cannot load a driver DLL by path directly —
        // it must be registered in the registry first. `ensure_dm8_driver_registered`
        // handles that at startup. Here we return the registered name.
        #[cfg(windows)]
        {
            return format!("{{{}}}", odbc_register::DM8_DRIVER_NAME);
        }

        // Linux / macOS: unixODBC supports specifying a .so path directly.
        #[cfg(not(windows))]
        {
            if let Ok(path) = std::env::var("DM8_DRIVER_PATH") {
                let p = path.trim().to_string();
                if !p.is_empty() {
                    tracing::debug!("DM8 driver from DM8_DRIVER_PATH: {}", p);
                    return format!("{{{}}}", p);
                }
            }

            let candidates = ["drivers/dm8/libdodbc.so", "../drivers/dm8/libdodbc.so"];
            for candidate in candidates {
                let path = std::path::Path::new(candidate);
                if path.exists() {
                    tracing::debug!("DM8 driver from bundled path: {}", candidate);
                    return format!("{{{}}}", path.display());
                }
            }

            tracing::warn!("DM8_DRIVER_PATH not set and no bundled driver found; falling back to registered name");
            "{DM8 ODBC DRIVER}".to_string()
        }
    }

    /// Builds the ODBC connection string expected by the DM8 driver.
    pub fn connection_string(&self) -> String {
        let driver = Self::driver_value();
        let cs = format!(
            "DRIVER={};SERVER={};PORT={};UID={};PWD={};CHARSET=1",
            driver, self.host, self.port, self.username, self.password
        );
        tracing::debug!(
            "ODBC connection string: DRIVER={};SERVER={};PORT={};UID={};PWD=***;CHARSET=1",
            driver, self.host, self.port, self.username
        );
        cs
    }

    /// Basic validation to surface misconfiguration early.
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.host.trim().is_empty(), "DM8 host is required");
        ensure!(self.port > 0, "DM8 port must be greater than zero");
        ensure!(!self.username.trim().is_empty(), "DM8 username is required");
        ensure!(!self.password.is_empty(), "DM8 password is required");
        Ok(())
    }
}

pub struct ConnectionPool {
    environment: Environment,
    connection_string: String,
    schema: Option<String>,
    display_dsn: String,
}

impl fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("dsn", &self.display_dsn)
            .field("schema", &self.schema)
            .finish()
    }
}

impl ConnectionPool {
    /// Create a new pool backed by the DM8 ODBC driver.
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        config
            .validate()
            .context("Invalid DM8 connection configuration")?;

        let environment = Environment::new().context("Failed to initialize ODBC environment")?;
        let connection_string = config.connection_string();
        let schema = if config.schema.trim().is_empty() {
            None
        } else {
            Some(config.schema)
        };

        Ok(Self {
            environment,
            display_dsn: format!("{}:{} as {}", config.host, config.port, config.username),
            connection_string,
            schema,
        })
    }

    /// Attempts to open a connection and run a lightweight query.
    pub fn test_connection(&self) -> Result<()> {
        let connection = self
            .get_connection()
            .context("Unable to open test connection to DM8")?;

        connection
            .execute("SELECT 1", ())
            .context("Connected to DM8 but failed to execute health query")?;

        Ok(())
    }

    /// Returns a new ODBC connection configured for DM8.
    pub fn get_connection(&self) -> Result<Connection<'_>> {
        let mut connection = self
            .environment
            .connect_with_connection_string(&self.connection_string, ConnectionOptions::default())
            .with_context(|| format!("Failed to connect to DM8 at {}", self.display_dsn))?;

        self.apply_schema(&mut connection)?;

        Ok(connection)
    }

    fn apply_schema(&self, connection: &mut Connection<'_>) -> Result<()> {
        if let Some(schema) = &self.schema {
            ensure!(
                is_valid_identifier(schema),
                "Invalid schema name '{}': must contain only letters, digits, underscores, $, or #",
                schema
            );
            let statement = format!("SET SCHEMA \"{}\"", schema.replace('"', "\"\""));
            connection.execute(&statement, ()).with_context(|| {
                format!("Connected to DM8 but failed to set schema to '{}'", schema)
            })?;
        }
        Ok(())
    }
}
