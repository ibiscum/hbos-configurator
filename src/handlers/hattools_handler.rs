//! HTTP handler exposing `configurator/hattools.py` functionality for the
//! `/api/v1/hat` endpoint.
use std::path::Path;

use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

use crate::api::hattools::{get_hat_info, SysfsHatEepromReader};

#[derive(Deserialize, Default)]
pub struct VerboseQuery {
    #[serde(default)]
    pub verbose: bool,
}

/// Handle GET /api/v1/hat - HAT vendor/product/UUID (from the device tree).
pub async fn handle_get_hat_info(Query(query): Query<VerboseQuery>) -> impl IntoResponse {
    let reader = SysfsHatEepromReader { root: Path::new("/") };
    let info = get_hat_info(&reader, query.verbose);
    (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": info })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    #[tokio::test]
    async fn handle_get_hat_info_returns_success_envelope() {
        let response = handle_get_hat_info(Query(VerboseQuery::default())).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["status"], "success");
        assert!(payload["result"].is_object());
    }
}
