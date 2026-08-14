//! Port of `configurator/handlers/` equivalent for the `/api/v1/systeminfo` endpoint.
use axum::{http::StatusCode, response::IntoResponse, Json};

use crate::api::systeminfo::SystemInfo;

/// Handle GET /api/v1/systeminfo - Pi model, HAT, sound card and system facts.
pub async fn handle_get_system_info() -> impl IntoResponse {
    let info = SystemInfo::new();
    let result = info.get_system_info_dict();
    (StatusCode::OK, Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    #[tokio::test]
    async fn handle_get_system_info_returns_success_envelope() {
        let response = handle_get_system_info().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["status"], "success");
        assert!(payload.get("pi_model").is_some());
        assert!(payload.get("hat_info").is_some());
        assert!(payload.get("soundcard").is_some());
        assert!(payload.get("system").is_some());
    }
}
