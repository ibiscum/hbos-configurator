//! HTTP handler exposing `configurator/sambamount.py` functionality (no
//! Python `handlers/` equivalent exists; endpoints follow the same
//! conventions as the other handlers).
use std::fs;
use std::sync::{LazyLock, Mutex};

use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::sambamount::{self, MemoryConfigDb, SystemCommandRunner};

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

// Mount configurations shared across requests, placeholder until ConfigDB is ported.
static CONFIG_DB: LazyLock<Mutex<MemoryConfigDb>> = LazyLock::new(|| Mutex::new(MemoryConfigDb::default()));

fn read_proc_mounts() -> String {
    fs::read_to_string("/proc/mounts").unwrap_or_default()
}

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

#[derive(Deserialize)]
pub struct AddMountPayload {
    server: String,
    share: String,
    mountpoint: Option<String>,
    user: Option<String>,
    password: Option<String>,
    version: Option<String>,
    options: Option<String>,
}

#[derive(Deserialize)]
pub struct ServerSharePayload {
    server: String,
    share: String,
}

/// Handle GET /api/v1/sambamount - list all configured mounts with status.
pub async fn handle_list_mounts() -> Response {
    let db = CONFIG_DB.lock().unwrap();
    let proc_mounts = read_proc_mounts();
    let mounts = sambamount::list_configured_mounts(&*db, &proc_mounts);
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(mounts),
        error: None,
    })
    .into_response()
}

/// Handle POST /api/v1/sambamount - add a mount configuration.
pub async fn handle_add_mount(Json(payload): Json<AddMountPayload>) -> Response {
    if payload.server.is_empty() || payload.share.is_empty() {
        return error_response("server and share are required", "invalid_request", StatusCode::BAD_REQUEST);
    }

    let mut db = CONFIG_DB.lock().unwrap();
    match sambamount::add_mount_config(
        &mut *db,
        &payload.server,
        &payload.share,
        payload.mountpoint.as_deref(),
        payload.user.as_deref(),
        payload.password.as_deref(),
        payload.version.as_deref(),
        payload.options.as_deref(),
    ) {
        Ok(()) => Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Added mount configuration for {}/{}", payload.server, payload.share)),
            data: None,
            error: None,
        })
        .into_response(),
        Err(msg) => error_response(&msg, "mount_config_exists", StatusCode::CONFLICT),
    }
}

/// Handle DELETE /api/v1/sambamount - remove a mount configuration.
pub async fn handle_remove_mount(Json(payload): Json<ServerSharePayload>) -> Response {
    let mut db = CONFIG_DB.lock().unwrap();
    match sambamount::remove_mount_config(&mut *db, &payload.server, &payload.share) {
        Ok(mountpoint) => Json(GenericResponse {
            status: ApiResponseStatus::Success,
            message: Some(format!("Removed mount configuration for {}/{}", payload.server, payload.share)),
            data: Some(serde_json::json!({ "mountpoint": mountpoint })),
            error: None,
        })
        .into_response(),
        Err(msg) => error_response(&msg, "mount_config_not_found", StatusCode::NOT_FOUND),
    }
}

/// Handle POST /api/v1/sambamount/mount - mount a specific share by server/share.
pub async fn handle_mount(Json(payload): Json<ServerSharePayload>) -> Response {
    let db = CONFIG_DB.lock().unwrap();
    let mounts = sambamount::read_mount_config(&*db);
    let runner = SystemCommandRunner;
    let proc_mounts = read_proc_mounts();

    match sambamount::mount_smb_share(&runner, &proc_mounts, &mounts, &payload.server, &payload.share) {
        Ok(()) => Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Mounted {}/{}", payload.server, payload.share)),
            data: None,
            error: None,
        })
        .into_response(),
        Err(msg) => error_response(&msg, "mount_failed", StatusCode::BAD_REQUEST),
    }
}

/// Handle POST /api/v1/sambamount/unmount - unmount a specific share by server/share.
pub async fn handle_unmount(Json(payload): Json<ServerSharePayload>) -> Response {
    let db = CONFIG_DB.lock().unwrap();
    let mounts = sambamount::read_mount_config(&*db);
    let runner = SystemCommandRunner;
    let proc_mounts = read_proc_mounts();

    match sambamount::unmount_smb_share(&runner, &proc_mounts, &mounts, &payload.server, &payload.share) {
        Ok(()) => Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Unmounted {}/{}", payload.server, payload.share)),
            data: None,
            error: None,
        })
        .into_response(),
        Err(msg) => error_response(&msg, "unmount_failed", StatusCode::BAD_REQUEST),
    }
}

/// Handle POST /api/v1/sambamount/mount-all - mount all configured shares.
pub async fn handle_mount_all() -> Response {
    let db = CONFIG_DB.lock().unwrap();
    let mounts = sambamount::read_mount_config(&*db);
    let runner = SystemCommandRunner;
    let proc_mounts = read_proc_mounts();

    let results = sambamount::mount_all_shares(&runner, &proc_mounts, &mounts);
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(results),
        error: None,
    })
    .into_response()
}

#[allow(dead_code)]
pub async fn handle_delete_mount_by_path(Path((server, share)): Path<(String, String)>) -> Response {
    handle_remove_mount(Json(ServerSharePayload { server, share })).await
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
    async fn list_mounts_returns_success_envelope() {
        let response = handle_list_mounts().await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"].is_array());
    }

    #[tokio::test]
    async fn add_mount_rejects_empty_server() {
        let response = handle_add_mount(Json(AddMountPayload {
            server: String::new(),
            share: "share".to_string(),
            mountpoint: None,
            user: None,
            password: None,
            version: None,
            options: None,
        }))
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remove_mount_reports_not_found_for_unknown_share() {
        let response = handle_remove_mount(Json(ServerSharePayload {
            server: "no-such-server".to_string(),
            share: "no-such-share".to_string(),
        }))
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn mount_reports_bad_request_for_unknown_config() {
        let response = handle_mount(Json(ServerSharePayload {
            server: "no-such-server".to_string(),
            share: "no-such-share".to_string(),
        }))
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mount_all_returns_success_envelope() {
        let response = handle_mount_all().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await["status"], "success");
    }
}
