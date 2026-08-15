//! HTTP handler exposing `configurator/dsptoolkit.py` functionality for the
//! `/api/v1/dsp/*` endpoints.
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

use crate::api::dsptoolkit::{DSPToolkit, ReqwestDspHttpClient, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT};

#[derive(Deserialize)]
pub struct DspQuery {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
}

fn default_host() -> String {
    DEFAULT_DSP_HOST.to_string()
}

fn default_port() -> u16 {
    DEFAULT_DSP_PORT
}

fn default_timeout() -> f64 {
    DEFAULT_TIMEOUT
}

impl Default for DspQuery {
    fn default() -> Self {
        Self { host: default_host(), port: default_port(), timeout: default_timeout() }
    }
}

/// Handle GET /api/v1/dsp/detect - full DSP detection payload.
pub async fn handle_detect(Query(query): Query<DspQuery>) -> impl IntoResponse {
    // reqwest::blocking builds its own runtime internally, so it must run
    // off the async executor thread to avoid a nested-runtime panic.
    let info = tokio::task::spawn_blocking(move || {
        let toolkit = DSPToolkit::new(&query.host, query.port, query.timeout);
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
        let toolkit = DSPToolkit::new(&query.host, query.port, query.timeout);
        toolkit.get_dsp_status(&ReqwestDspHttpClient)
    })
    .await
    .unwrap_or_else(|_| "error".to_string());

    (StatusCode::OK, Json(serde_json::json!({ "status": "success", "result": { "status": status } })))
}

/// Handle GET /api/v1/dsp/name - detected DSP name, or 404 if none detected.
pub async fn handle_name(Query(query): Query<DspQuery>) -> axum::response::Response {
    let name = tokio::task::spawn_blocking(move || {
        let toolkit = DSPToolkit::new(&query.host, query.port, query.timeout);
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
    use axum::body::to_bytes;
    use serde_json::Value;

    async fn body_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    // No real DSP service runs in tests, so every endpoint exercises the
    // "unavailable"/not-detected path against an unreachable loopback port.
    fn unreachable_query() -> Query<DspQuery> {
        Query(DspQuery { host: "127.0.0.1".to_string(), port: 1, timeout: 0.5 })
    }

    #[tokio::test]
    async fn detect_returns_unavailable_when_service_unreachable() {
        let response = handle_detect(unreachable_query()).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert_eq!(payload["result"]["status"], "unavailable");
    }

    #[tokio::test]
    async fn status_returns_unavailable_when_service_unreachable() {
        let response = handle_status(unreachable_query()).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["result"]["status"], "unavailable");
    }

    #[tokio::test]
    async fn name_returns_not_found_when_service_unreachable() {
        let response = handle_name(unreachable_query()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_json(response).await["status"], "error");
    }
}
