use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub struct Error(pub StatusCode, pub &'static str);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.0, self.1)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn bad(code: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, code)
    }
    pub fn conflict(code: &'static str) -> Self {
        Self(StatusCode::CONFLICT, code)
    }
    pub fn denied() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found")
    }
    pub fn auth() -> Self {
        Self(StatusCode::UNAUTHORIZED, "authentication_required")
    }
    pub fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":{"code":self.1}}))).into_response()
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref d) = e {
            if d.is_unique_violation() {
                return Self::conflict("already_exists");
            }
            if d.is_foreign_key_violation() {
                return Self::conflict("referenced_resource_changed");
            }
        }
        tracing::error!(error=%e, "database operation failed");
        Self::unavailable()
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        tracing::error!(error=%e, "storage operation failed");
        if matches!(e.raw_os_error(), Some(28 | 112 | 39)) {
            Self(StatusCode::INSUFFICIENT_STORAGE, "storage_full")
        } else {
            Self::unavailable()
        }
    }
}
