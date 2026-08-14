//! HTTP handler exposing `configurator/systemd_service.py` functionality.
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::api::systemd_service::SystemdServiceManager;
use crate::api::wifi::SystemCommandRunner;

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

#[derive(Deserialize)]
pub struct ListServicesQuery {
    pub pattern: Option<String>,
}

fn error_response(msg: String, code: &str, status: StatusCode) -> Response {
    error!("{}", msg);
    (
        status,
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Error,
            message: Some(msg),
            data: None,
            error: Some(code.to_string()),
        }),
    )
        .into_response()
}

fn message_response(success: bool, message: String) -> Response {
    if success {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(message),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response(message, "systemd_action_failed", StatusCode::BAD_REQUEST)
    }
}

pub async fn handle_enable(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.enable(&service);
    message_response(success, message)
}

pub async fn handle_disable(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.disable(&service);
    message_response(success, message)
}

pub async fn handle_start(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.start(&service);
    message_response(success, message)
}

pub async fn handle_stop(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.stop(&service);
    message_response(success, message)
}

pub async fn handle_restart(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.restart(&service);
    message_response(success, message)
}

pub async fn handle_reload(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.reload(&service);
    message_response(success, message)
}

pub async fn handle_enable_now(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.enable_now(&service);
    message_response(success, message)
}

pub async fn handle_disable_now(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.disable_now(&service);
    message_response(success, message)
}

pub async fn handle_status(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, status) = manager.status(&service);
    if success {
        Json(GenericResponse {
            status: ApiResponseStatus::Success,
            message: None,
            data: Some(status),
            error: None,
        })
        .into_response()
    } else {
        error_response(
            format!("Failed to get status for service '{}'", service),
            "systemd_status_unavailable",
            StatusCode::NOT_FOUND,
        )
    }
}

pub async fn handle_is_active(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let active = manager.is_active(&service);
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(serde_json::json!({ "active": active })),
        error: None,
    })
    .into_response()
}

pub async fn handle_is_enabled(Path(service): Path<String>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let enabled = manager.is_enabled(&service);
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(serde_json::json!({ "enabled": enabled })),
        error: None,
    })
    .into_response()
}

pub async fn handle_list_services(Query(query): Query<ListServicesQuery>) -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, services) = manager.list_services(query.pattern.as_deref());
    if success {
        Json(GenericResponse {
            status: ApiResponseStatus::Success,
            message: None,
            data: Some(serde_json::json!({ "services": services, "count": services.len() })),
            error: None,
        })
        .into_response()
    } else {
        error_response(
            "Failed to list systemd services".to_string(),
            "systemd_list_failed",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    }
}

pub async fn handle_daemon_reload() -> Response {
    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let (success, message) = manager.daemon_reload();
    message_response(success, message)
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
    async fn enable_reports_failure_without_systemd() {
        // No real systemd/systemctl guaranteed in the test environment, but an
        // unknown/invalid service name always fails validation deterministically.
        let response = handle_enable(Path("../not-a-service".to_string())).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "error");
        assert_eq!(payload["error"], "systemd_action_failed");
    }

    #[tokio::test]
    async fn is_active_returns_success_envelope() {
        let response = handle_is_active(Path("definitely-not-a-real-service".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert_eq!(payload["data"]["active"], false);
    }
}
