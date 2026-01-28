use anyhow::{ensure, Context, Result};
use odbc_api::{Connection, ConnectionOptions, Environment};
use std::fmt;

use crate::models::ConnectionConfig;

impl ConnectionConfig {
    /// Returns the ODBC driver value; prefers an explicit path from `DM8_DRIVER_PATH`.
    fn driver_value() -> String {
        if let Ok(path) = std::env::var("DM8_DRIVER_PATH") {
            if !path.trim().is_empty() {
                return format!("{{{}}}", path.trim());
            }
        }

        // Try bundled relative path (for HTTP dev runs)
        let candidates = [
            "drivers/dm8/libdodbc.so",
            "../drivers/dm8/libdodbc.so",
        ];
        for candidate in candidates {
            let path = std::path::Path::new(candidate);
            if path.exists() {
                return format!("{{{}}}", path.display());
            }
        }

        "{DM8 ODBC DRIVER}".to_string()
    }

    /// Builds the ODBC connection string expected by the DM8 driver.
    pub fn connection_string(&self) -> String {
        let driver = Self::driver_value();
        format!(
            "DRIVER={};SERVER={};PORT={};UID={};PWD={}",
            driver, self.host, self.port, self.username, self.password
        )
    }

    /// Basic validation to surface misconfiguration early.
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.host.trim().is_empty(), "DM8 host is required");
        ensure!(self.port > 0, "DM8 port must be greater than zero");
        ensure!(
            !self.username.trim().is_empty(),
            "DM8 username is required"
        );
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
            .connect_with_connection_string(
                &self.connection_string,
                ConnectionOptions::default(),
            )
            .with_context(|| format!("Failed to connect to DM8 at {}", self.display_dsn))?;

        self.apply_schema(&mut connection)?;

        Ok(connection)
    }

    fn apply_schema(&self, connection: &mut Connection<'_>) -> Result<()> {
        if let Some(schema) = &self.schema {
            let statement = format!("SET SCHEMA {}", schema);
            connection
                .execute(&statement, ())
                .with_context(|| format!("Connected to DM8 but failed to set schema to '{}'", schema))?;
        }
        Ok(())
    }
}
