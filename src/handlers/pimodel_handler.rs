//! HTTP handler exposing `configurator/pimodel.py` functionality for the
//! `/api/v1/pimodel` endpoint.
use axum::{http::StatusCode, response::IntoResponse, Json};

use crate::api::pimodel::PiModel;

/// Handle GET /api/v1/pimodel - detected Raspberry Pi model name and version.
pub async fn handle_get_pi_model() -> impl IntoResponse {
    let model = PiModel::new();
    (StatusCode::OK, Json(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    #[tokio::test]
    async fn handle_get_pi_model_returns_model_and_version() {
        let response = handle_get_pi_model().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert!(payload.get("model_name").is_some());
        assert!(payload.get("version").is_some());
    }
}
