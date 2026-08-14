//! HTTP handler exposing `configurator/wifi.py` functionality (no Python equivalent
//! file exists; endpoints follow the same conventions as the other handlers).
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::api::wifi::{self, SystemCommandRunner};

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ApiResponseStatus {
    Success,
    Error,
}

#[derive(Serialize)]
struct GenericResponse<T> {
    status: ApiResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct NetworksData {
    networks: Vec<wifi::WifiNetwork>,
    count: usize,
}

#[derive(Deserialize)]
pub struct ConnectPayload {
    ssid: String,
    passphrase: Option<String>,
    #[serde(default)]
    revert_when_fail: bool,
}

fn error_response(msg: &str, code: &str, status: StatusCode) -> Response {
    error!("{}", msg);
    (
        status,
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Error,
            message: Some(msg.to_string()),
            data: None,
            error: Some(code.to_string()),
        }),
    )
        .into_response()
}

pub async fn handle_list_networks() -> Response {
    let runner = SystemCommandRunner;
    let networks = wifi::scan_wifi_networks(&runner);
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(NetworksData {
            count: networks.len(),
            networks,
        }),
        error: None,
    })
    .into_response()
}

pub async fn handle_get_current_connection() -> Response {
    let runner = SystemCommandRunner;
    match wifi::get_current_connection(&runner) {
        Some(conn) => Json(GenericResponse {
            status: ApiResponseStatus::Success,
            message: None,
            data: Some(conn),
            error: None,
        })
        .into_response(),
        None => error_response(
            "Not currently connected to any WiFi network",
            "wifi_not_connected",
            StatusCode::NOT_FOUND,
        ),
    }
}

pub async fn handle_connect(Json(payload): Json<ConnectPayload>) -> Response {
    if payload.ssid.trim().is_empty() {
        return error_response("ssid parameter is required", "missing_ssid", StatusCode::BAD_REQUEST);
    }

    let runner = SystemCommandRunner;
    let ok = wifi::connect_to_wifi(
        &runner,
        &payload.ssid,
        payload.passphrase.as_deref(),
        payload.revert_when_fail,
    );

    if ok {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Connected to {}", payload.ssid)),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response(
            &format!("Failed to connect to {}", payload.ssid),
            "wifi_connect_failed",
            StatusCode::BAD_REQUEST,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    async fn body_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn list_networks_returns_success_envelope() {
        let response = handle_list_networks().await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"]["networks"].is_array());
    }

    #[tokio::test]
    async fn get_current_connection_reports_not_found_without_hardware() {
        // No real WiFi hardware/NetworkManager in the test environment.
        let response = handle_get_current_connection().await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let payload = body_json(response).await;
        assert_eq!(payload["error"], "wifi_not_connected");
    }

    #[tokio::test]
    async fn connect_rejects_empty_ssid() {
        let response = handle_connect(Json(ConnectPayload {
            ssid: String::new(),
            passphrase: None,
            revert_when_fail: false,
        }))
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = body_json(response).await;
        assert_eq!(payload["error"], "missing_ssid");
    }
}
