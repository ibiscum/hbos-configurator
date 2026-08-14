//! HTTP handler exposing `configurator/hostname_utils.py` functionality (no
//! Python `handlers/` equivalent exists; endpoints follow the same
//! conventions as the other handlers).
use std::path::Path;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

use crate::api::hostconfig::SystemCommandRunner;
use crate::api::hostname_utils;

/// Handle GET /api/v1/hostname - current hostname and pretty hostname (with fallback).
pub async fn handle_get_hostname() -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let (hostname, pretty_hostname) = hostname_utils::get_hostnames_with_fallback(&runner);
    (StatusCode::OK, Json(serde_json::json!({ "status": "success", "hostname": hostname, "pretty_hostname": pretty_hostname })))
}

#[derive(Deserialize)]
pub struct SetHostnamePayload {
    hostname: String,
}

/// Handle POST /api/v1/hostname - validate and set the system hostname.
pub async fn handle_set_hostname(Json(payload): Json<SetHostnamePayload>) -> axum::response::Response {
    if !hostname_utils::validate_hostname(&payload.hostname) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": format!("'{}' is not a valid hostname", payload.hostname) }))).into_response();
    }

    let runner = SystemCommandRunner;
    if hostname_utils::set_hostname(&runner, Path::new("/"), &payload.hostname) {
        (StatusCode::OK, Json(serde_json::json!({ "status": "success", "message": format!("Successfully set hostname to '{}'", payload.hostname) }))).into_response()
    } else {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": "Failed to set hostname" }))).into_response()
    }
}

#[derive(Deserialize)]
pub struct SetPrettyHostnamePayload {
    pretty_hostname: String,
}

/// Handle POST /api/v1/hostname/pretty - validate and set the pretty hostname.
pub async fn handle_set_pretty_hostname(Json(payload): Json<SetPrettyHostnamePayload>) -> axum::response::Response {
    if !hostname_utils::validate_pretty_hostname(&payload.pretty_hostname) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": "Invalid pretty hostname" }))).into_response();
    }

    let runner = SystemCommandRunner;
    if hostname_utils::set_pretty_hostname(&runner, &payload.pretty_hostname) {
        (StatusCode::OK, Json(serde_json::json!({ "status": "success", "message": format!("Successfully set pretty hostname to '{}'", payload.pretty_hostname) }))).into_response()
    } else {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "status": "error", "error": "Failed to set pretty hostname" }))).into_response()
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
    async fn get_hostname_returns_success_envelope() {
        let response = handle_get_hostname().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
    }

    #[tokio::test]
    async fn set_hostname_rejects_invalid_format() {
        let response = handle_set_hostname(Json(SetHostnamePayload { hostname: "-bad-hostname".to_string() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["status"], "error");
    }

    #[tokio::test]
    async fn set_pretty_hostname_rejects_non_printable() {
        let response = handle_set_pretty_hostname(Json(SetPrettyHostnamePayload { pretty_hostname: "HiFi\nBerry".to_string() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
