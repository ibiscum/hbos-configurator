//! HTTP handler exposing `configurator/i2c.py` functionality (no Python
//! `handlers/` equivalent exists; endpoints follow the same conventions as
//! the other handlers).
use std::path::Path;

use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

use crate::api::i2c::{self, SystemCommandRunner};

#[derive(Deserialize)]
pub struct BusQuery {
    #[serde(default = "default_bus")]
    pub bus: i32,
}

fn default_bus() -> i32 {
    1
}

/// Handle GET /api/v1/i2c/scan?bus=1 - scan an I2C bus for devices.
pub async fn handle_scan(Query(query): Query<BusQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    let bus_number = match i2c::validate_bus_number(query.bus) {
        Ok(n) => n,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": e }))).into_response(),
    };

    match i2c::scan_i2c_bus(&runner, bus_number) {
        Ok(result) => (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": result }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": e }))).into_response(),
    }
}

/// Handle GET /api/v1/i2c/info?bus=1 - I2C bus info, including scan results if available.
pub async fn handle_info(Query(query): Query<BusQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    match i2c::get_i2c_info(&runner, Path::new("/"), query.bus) {
        Ok(info) => (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": info }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": e }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    async fn body_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn scan_rejects_invalid_bus_number() {
        let response = handle_scan(Query(BusQuery { bus: 42 })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["status"], "error");
    }

    #[tokio::test]
    async fn info_returns_success_envelope_for_valid_bus() {
        let response = handle_info(Query(BusQuery { bus: 1 })).await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["result"]["bus_exists"].is_boolean());
    }

    #[tokio::test]
    async fn info_rejects_invalid_bus_number() {
        let response = handle_info(Query(BusQuery { bus: -5 })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
