//! HTTP handler exposing `configurator/dsptoolkit.py` functionality for the
//! `/api/v1/dsp/*` endpoints.
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

use crate::api::dsptoolkit::{DSPToolkit, ReqwestDspHttpClient, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT};

#[derive(Deserialize)]
pub struct DspQuery {
    #[serde(default = "default_timeout")]
    pub timeout: f64,
}

fn default_timeout() -> f64 {
    DEFAULT_TIMEOUT
}

impl Default for DspQuery {
    fn default() -> Self {
        Self { timeout: default_timeout() }
    }
}

fn dsp_toolkit(timeout: f64) -> DSPToolkit {
    DSPToolkit::new(DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, timeout)
}

/// Handle GET /api/v1/dsp/detect - full DSP detection payload.
pub async fn handle_detect(Query(query): Query<DspQuery>) -> impl IntoResponse {
    // reqwest::blocking builds its own runtime internally, so it must run
    // off the async executor thread to avoid a nested-runtime panic.
    let info = tokio::task::spawn_blocking(move || {
        let toolkit = dsp_toolkit(query.timeout);
        toolkit.detect_dsp(&ReqwestDspHttpClient)
    })
    .await
    .unwrap_or(None);

    match info {
        Some(info) => (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": info }))),
        None => (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": { "status": "unavailable" } }))),
    }
}

/// Handle GET /api/v1/dsp/status - DSP detection status only.
pub async fn handle_status(Query(query): Query<DspQuery>) -> impl IntoResponse {
    let status = tokio::task::spawn_blocking(move || {
        let toolkit = dsp_toolkit(query.timeout);
        toolkit.get_dsp_status(&ReqwestDspHttpClient)
    })
    .await
    .unwrap_or_else(|_| "error".to_string());

    (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": { "status": status } })))
}

/// Handle GET /api/v1/dsp/name - detected DSP name, or 404 if none detected.
pub async fn handle_name(Query(query): Query<DspQuery>) -> axum::response::Response {
    let name = tokio::task::spawn_blocking(move || {
        let toolkit = dsp_toolkit(query.timeout);
        toolkit.get_detected_dsp_name(&ReqwestDspHttpClient)
    })
    .await
    .unwrap_or(None);

    match name {
        Some(name) => (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": { "name": name } }))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "status": "error", "error": "no DSP detected" }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_api_always_targets_the_configured_local_dsp_service() {
        let query: DspQuery =
            serde_json::from_str(r#"{"host":"169.254.169.254","port":80,"timeout":1.0}"#).unwrap();
        let toolkit = dsp_toolkit(query.timeout);
        assert_eq!(toolkit.host, DEFAULT_DSP_HOST);
        assert_eq!(toolkit.port, DEFAULT_DSP_PORT);
    }
}
