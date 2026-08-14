//! HTTP handler exposing `configurator/pipewire.py` functionality (no Python
//! `handlers/` equivalent exists; endpoints follow the same conventions as
//! the other handlers).
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::api::pipewire::{self, SystemCommandRunner};

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

fn error_response(msg: &str, code: &str, status: StatusCode) -> axum::response::Response {
    (status, Json(GenericResponse::<()> { status: ApiResponseStatus::Error, message: Some(msg.to_string()), data: None, error: Some(code.to_string()) }))
        .into_response()
}

/// Handle GET /api/v1/pipewire/controls - list all PipeWire volume control names.
pub async fn handle_list_controls() -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let controls = pipewire::get_volume_controls(&runner);
    (StatusCode::OK, Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(controls), error: None }))
}

#[derive(Deserialize)]
pub struct ControlQuery {
    pub control: String,
}

/// Handle GET /api/v1/pipewire/volume?control=... - get a control's current volume.
pub async fn handle_get_volume(Query(query): Query<ControlQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    match pipewire::get_volume(&runner, &query.control) {
        Some(volume) => Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(serde_json::json!({ "control": query.control, "volume": volume })), error: None })
            .into_response(),
        None => error_response(&format!("Control '{}' not found or no volume info", query.control), "control_not_found", StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct SetVolumePayload {
    control: String,
    volume: f64,
}

/// Handle POST /api/v1/pipewire/volume - set a control's volume (0.0-1.0).
pub async fn handle_set_volume(Json(payload): Json<SetVolumePayload>) -> axum::response::Response {
    if !payload.volume.is_finite() || !(0.0..=1.0).contains(&payload.volume) {
        return error_response("Volume must be a float between 0.0 and 1.0", "invalid_volume", StatusCode::BAD_REQUEST);
    }

    let runner = SystemCommandRunner;
    if pipewire::set_volume(&runner, &payload.control, payload.volume) {
        Json(GenericResponse::<()> { status: ApiResponseStatus::Success, message: Some(format!("Volume set for '{}'", payload.control)), data: None, error: None }).into_response()
    } else {
        error_response(&format!("Failed to set volume for '{}'", payload.control), "set_volume_failed", StatusCode::BAD_REQUEST)
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
    async fn list_controls_returns_success_envelope() {
        let response = handle_list_controls().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"].is_array());
    }

    #[tokio::test]
    async fn get_volume_reports_not_found_without_pw_cli() {
        let response = handle_get_volume(Query(ControlQuery { control: "Master".to_string() })).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_volume_rejects_out_of_range_value() {
        let response = handle_set_volume(Json(SetVolumePayload { control: "Master".to_string(), volume: 1.5 })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["status"], "error");
    }
}
