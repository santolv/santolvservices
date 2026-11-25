use crate::{App, error::Result, storage};
use axum::Json;
use axum::{extract::State, http::StatusCode};

pub fn default_frontend_origin() -> String {
    "http://localhost:5500".into()
}

pub async fn index() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service":"Santolv Services API", "health":"/health/live"}))
}

pub async fn health(State(s): State<App>) -> Result<StatusCode> {
    sqlx::query("SELECT 1").execute(&s.db).await?;
    storage::capacity(&s.root, 0).await?;
    Ok(StatusCode::NO_CONTENT)
}
