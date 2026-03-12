use axum::{http::StatusCode, Json};

use crate::models::{ApiResponse, ErrorCode};

pub type ApiResult<T> = Result<Json<ApiResponse<T>>, (StatusCode, Json<ApiResponse<T>>)>;

pub fn ok<T>(data: T) -> ApiResult<T> {
    Ok(Json(ApiResponse::success(data)))
}

pub fn err<T>(status: StatusCode, message: impl Into<String>) -> ApiResult<T> {
    Err((status, Json(ApiResponse::error(message.into()))))
}

pub fn err_with_code<T>(
    status: StatusCode,
    message: impl Into<String>,
    code: ErrorCode,
) -> ApiResult<T> {
    Err((
        status,
        Json(ApiResponse::error_with_code(message.into(), code)),
    ))
}
