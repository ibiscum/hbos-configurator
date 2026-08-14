//! HTTP handler exposing `configurator/settings_manager.py` functionality
//! (no Python `handlers/` equivalent exists; endpoints follow the same
//! conventions as the other handlers).
use std::sync::{LazyLock, Mutex};

use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::api::settings_manager::{MemoryConfigDb, SettingsManager};

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

// Settings manager shared across requests. No modules register real
// save/restore callbacks yet, so this currently only tracks saved values
// (list/delete); registration will grow as other modules are ported.
static SETTINGS_MANAGER: LazyLock<Mutex<SettingsManager>> =
    LazyLock::new(|| Mutex::new(SettingsManager::new(Box::new(MemoryConfigDb::default()))));

fn error_response(msg: &str, code: &str, status: StatusCode) -> Response {
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

/// Handle GET /api/v1/settings - list registered and saved setting names.
pub async fn handle_list_settings() -> Response {
    let mgr = SETTINGS_MANAGER.lock().unwrap();
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(serde_json::json!({
            "registered": mgr.list_registered_settings(),
            "saved": mgr.list_saved_settings(),
        })),
        error: None,
    })
    .into_response()
}

/// Handle POST /api/v1/settings/save - save all registered settings.
pub async fn handle_save_all_settings() -> Response {
    let mut mgr = SETTINGS_MANAGER.lock().unwrap();
    let results = mgr.save_all_settings();
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(results),
        error: None,
    })
    .into_response()
}

/// Handle POST /api/v1/settings/restore - restore all registered settings.
pub async fn handle_restore_all_settings() -> Response {
    let mut mgr = SETTINGS_MANAGER.lock().unwrap();
    let results = mgr.restore_all_settings();
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(results),
        error: None,
    })
    .into_response()
}

/// Handle DELETE /api/v1/settings/:name - delete a saved setting.
pub async fn handle_delete_setting(Path(name): Path<String>) -> Response {
    let mut mgr = SETTINGS_MANAGER.lock().unwrap();
    if mgr.delete_saved_setting(&name) {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Deleted saved setting '{name}'")),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response("Setting name must not be empty", "invalid_setting_name", StatusCode::BAD_REQUEST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    async fn body_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn list_settings_returns_success_envelope() {
        let response = handle_list_settings().await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"]["registered"].is_array());
        assert!(payload["data"]["saved"].is_object());
    }

    #[tokio::test]
    async fn save_all_and_restore_all_return_success_envelope() {
        let save_response = handle_save_all_settings().await;
        assert_eq!(save_response.status(), StatusCode::OK);
        assert_eq!(body_json(save_response).await["status"], "success");

        let restore_response = handle_restore_all_settings().await;
        assert_eq!(restore_response.status(), StatusCode::OK);
        assert_eq!(body_json(restore_response).await["status"], "success");
    }

    #[tokio::test]
    async fn delete_setting_rejects_empty_name() {
        let response = handle_delete_setting(Path(String::new())).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["status"], "error");
    }

    #[tokio::test]
    async fn delete_setting_accepts_name() {
        let response = handle_delete_setting(Path("some-setting".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await["status"], "success");
    }
}
