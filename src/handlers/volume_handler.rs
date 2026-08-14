//! Port of `configurator/handlers/volume_handler.py`.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use tracing::error;

use crate::api::volume::{self, AlsaVolumeBackend, MemoryConfigStore};

// --- DATENSTRUKTUREN ---
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
pub struct SetVolumePayload {
    volume: serde_json::Value,
}

#[derive(Serialize)]
struct ControlsData {
    controls: Vec<String>,
    count: usize,
}

#[derive(Serialize)]
struct VolumeData {
    volume: i32,
    control: String,
}

// Config store shared across requests, placeholder until ConfigDB is ported.
static CONFIG_STORE: LazyLock<Mutex<MemoryConfigStore>> =
    LazyLock::new(|| Mutex::new(MemoryConfigStore::default()));

/// Sound card index used for headphone control, placeholder until soundcard detection is ported.
fn default_card_index() -> Option<i32> {
    Some(0)
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

// --- API HANDLER (Async) ---

pub async fn handle_list_headphone_controls() -> Response {
    let backend = AlsaVolumeBackend;
    let controls = volume::get_available_headphone_controls(&backend, default_card_index());
    Json(GenericResponse {
        status: ApiResponseStatus::Success,
        message: None,
        data: Some(ControlsData {
            count: controls.len(),
            controls,
        }),
        error: None,
    })
    .into_response()
}

pub async fn handle_get_headphone_volume() -> Response {
    let backend = AlsaVolumeBackend;
    match volume::get_headphone_volume(&backend, default_card_index()) {
        (Some(vol_str), Some(ctrl)) => {
            let vol = vol_str.parse::<f64>().unwrap_or(0.0).round() as i32;
            Json(GenericResponse {
                status: ApiResponseStatus::Success,
                message: None,
                data: Some(VolumeData {
                    volume: vol,
                    control: ctrl,
                }),
                error: None,
            })
            .into_response()
        }
        _ => error_response(
            "No headphone volume controls available on this sound card",
            "headphone_control_not_found",
            StatusCode::NOT_FOUND,
        ),
    }
}

pub async fn handle_set_headphone_volume(Json(payload): Json<SetVolumePayload>) -> Response {
    let vol_int = match payload.volume {
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(v) => v as i32,
            None => {
                return error_response(
                    "Volume must be a valid integer",
                    "invalid_volume_type",
                    StatusCode::BAD_REQUEST,
                )
            }
        },
        serde_json::Value::String(ref s) => match s.parse::<i32>() {
            Ok(v) => v,
            Err(_) => {
                return error_response(
                    "Volume must be a valid integer",
                    "invalid_volume_type",
                    StatusCode::BAD_REQUEST,
                )
            }
        },
        _ => {
            return error_response(
                "Volume must be a valid integer",
                "invalid_volume_type",
                StatusCode::BAD_REQUEST,
            )
        }
    };

    if !(0..=100).contains(&vol_int) {
        return error_response(
            "Volume must be between 0 and 100",
            "invalid_volume_range",
            StatusCode::BAD_REQUEST,
        );
    }

    let backend = AlsaVolumeBackend;
    if volume::set_headphone_volume(&backend, default_card_index(), &vol_int.to_string()) {
        Json(GenericResponse {
            status: ApiResponseStatus::Success,
            message: Some(format!("Headphone volume set to {}%", vol_int)),
            data: Some(serde_json::json!({ "volume": vol_int })),
            error: None,
        })
        .into_response()
    } else {
        error_response(
            "No headphone volume controls available on this sound card",
            "headphone_control_not_found",
            StatusCode::NOT_FOUND,
        )
    }
}

pub async fn handle_store_headphone_volume() -> Response {
    let backend = AlsaVolumeBackend;
    let mut store = CONFIG_STORE.lock().unwrap();
    if volume::store_headphone_volume(&backend, &mut *store, default_card_index()) {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some("Headphone volume stored successfully".to_string()),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response(
            "No headphone volume controls available on this sound card",
            "headphone_control_not_found",
            StatusCode::NOT_FOUND,
        )
    }
}

pub async fn handle_restore_headphone_volume() -> Response {
    let backend = AlsaVolumeBackend;
    let store = CONFIG_STORE.lock().unwrap();
    if volume::restore_headphone_volume(&backend, &*store, default_card_index()) {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some("Headphone volume restored successfully".to_string()),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response(
            "No headphone volume settings found or no compatible controls available",
            "headphone_volume_restore_source_not_found",
            StatusCode::NOT_FOUND,
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
    async fn list_headphone_controls_returns_success_envelope() {
        let response = handle_list_headphone_controls().await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"]["controls"].is_array());
        assert!(payload["data"]["count"].is_number());
    }

    #[tokio::test]
    async fn set_headphone_volume_rejects_negative_value() {
        let response =
            handle_set_headphone_volume(Json(SetVolumePayload { volume: serde_json::json!(-10) }))
                .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "error");
        assert_eq!(payload["error"], "invalid_volume_range");
    }

    #[tokio::test]
    async fn set_headphone_volume_rejects_value_above_100() {
        let response =
            handle_set_headphone_volume(Json(SetVolumePayload { volume: serde_json::json!(150) }))
                .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_headphone_volume_rejects_non_numeric_value() {
        let response = handle_set_headphone_volume(Json(SetVolumePayload {
            volume: serde_json::json!("not-a-number"),
        }))
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = body_json(response).await;
        assert_eq!(payload["error"], "invalid_volume_type");
    }

    #[tokio::test]
    async fn store_and_restore_roundtrip_without_hardware_reports_not_found() {
        // No real ALSA hardware/controls exist in the test environment, so both
        // operations should surface the documented 404 "not found" error path.
        let response = handle_store_headphone_volume().await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = handle_restore_headphone_volume().await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

