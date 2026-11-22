use axum::Json;

pub fn default_frontend_origin() -> String {
    "http://localhost:5500".into()
}

pub async fn index() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service":"SantolvDrive API", "health":"/health/live"}))
}
