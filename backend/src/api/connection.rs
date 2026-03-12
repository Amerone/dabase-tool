use axum::{extract::Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    api::response::{self, ApiResult},
    db::service,
    models::{ConnectionConfig, DbType, ErrorCode},
};

#[derive(Debug, Deserialize)]
pub struct TestConnectionRequest {
    #[serde(default)]
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub schema: String,
}

#[derive(Debug, Serialize)]
pub struct TestConnectionResponse {
    pub success: bool,
    pub message: String,
}

pub async fn test_connection(
    Json(req): Json<TestConnectionRequest>,
) -> ApiResult<TestConnectionResponse> {
    let config = ConnectionConfig {
        db_type: req.db_type,
        host: req.host,
        port: req.port,
        username: req.username,
        password: req.password,
        schema: req.schema,
        export_schema: None,
    };

    match service::test_connection(&config).await {
        Ok(_) => response::ok(TestConnectionResponse {
            success: true,
            message: "Connection successful".to_string(),
        }),
        Err(e) => {
            error!(error = ?e, "Database connection test failed");
            response::err_with_code(
                StatusCode::BAD_REQUEST,
                "Database connection test failed. Verify host/port/credentials and driver configuration.",
                ErrorCode::DatabaseConnection,
            )
        }
    }
}
