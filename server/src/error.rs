use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

/// Errors the API returns. Messages are written for people, never internal details.
#[derive(Debug)]
pub enum AppError {
    BadRequest(&'static str),
    Unauthorized(&'static str),
    Forbidden,
    NotFound,
    Conflict(&'static str),
    TooMany,
    /// A conflict the app can resolve, with details (e.g. overlapping appointments to show before "Book Anyway").
    ConflictWith(&'static str, &'static str, serde_json::Value),
    /// Refused because of the device (phase 3), with a code the app acts on: "device_revoked", "app_update".
    DeviceRefused(&'static str, &'static str),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, msg) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m.to_string()),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, "unauthorized", m.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Your role can’t do this.".into()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found", "Not found.".into()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m.to_string()),
            AppError::TooMany => (StatusCode::TOO_MANY_REQUESTS, "too_many", "Too many attempts. Try again in 15 minutes.".into()),
            AppError::ConflictWith(code, m, details) => {
                return (StatusCode::CONFLICT, Json(json!({ "error": code, "message": m, "details": details }))).into_response();
            }
            AppError::DeviceRefused(code, m) => {
                return (StatusCode::FORBIDDEN, Json(json!({ "error": code, "message": m }))).into_response();
            }
            AppError::Internal(e) => {
                tracing::error!("internal error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong on the server.".into())
            }
        };
        (status, Json(json!({ "error": code, "message": msg }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        // Bad input that the database caught (wrong date, unknown id, missing field, broken rule):
        // answer in plain words and don't log it — the database's own message can contain patient data.
        if let sqlx::Error::Database(d) = &e {
            match d.code().as_deref() {
                Some("22007" | "22008" | "22P02" | "22003" | "22023") => return AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers."),
                Some("23502" | "23514") => return AppError::BadRequest("Some required details are missing or not allowed."),
                Some("23503") => return AppError::BadRequest("Something this refers to doesn’t exist any more. Refresh and try again."),
                Some("23505") => return AppError::Conflict("This already exists."),
                Some("P0001") => return AppError::Conflict("This record is locked and can’t be changed."),
                Some(code) => return AppError::Internal(format!("database error {code}")),
                None => {}
            }
        }
        AppError::Internal(e.to_string())
    }
}

pub type ApiResult<T> = Result<T, AppError>;
