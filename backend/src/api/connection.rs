use axum::{extract::Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    db::connection::ConnectionPool,
    models::{ApiResponse, ConnectionConfig},
};

#[derive(Debug, Deserialize)]
pub struct TestConnectionRequest {
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
) -> Result<Json<ApiResponse<TestConnectionResponse>>, StatusCode> {
    let config = ConnectionConfig {
        host: req.host,
        port: req.port,
        username: req.username,
        password: req.password,
        schema: req.schema,
        export_schema: None,
    };

    match ConnectionPool::new(config) {
        Ok(pool) => match pool.test_connection() {
            Ok(_) => Ok(Json(ApiResponse::success(TestConnectionResponse {
                success: true,
                message: "Connection successful".to_string(),
            }))),
            Err(e) => {
                let detailed_error = format!("{:#}", e);
                error!("DM8 connection test failed: {}", detailed_error);
                Ok(Json(ApiResponse::error(format!(
                    "Connection test failed: {}",
                    detailed_error
                ))))
            }
        },
        Err(e) => {
            let detailed_error = format!("{:#}", e);
            error!("Failed to create DM8 connection pool: {}", detailed_error);
            Ok(Json(ApiResponse::error(format!(
                "Failed to create connection pool: {}",
                detailed_error
            ))))
        }
    }
}
