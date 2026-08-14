//! HTTP handler exposing `configurator/soundcard_detector.py` functionality
//! for the `/api/v1/soundcard/detect` endpoint.
use std::fs;
use std::path::PathBuf;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use serde_json::json;

use crate::api::soundcard_detector::SoundcardDetector;
use crate::api::systeminfo::read_hat_info;

const CONFIG_TXT_PATH: &str = "/boot/firmware/config.txt";
const REBOOT_FILE_PATH: &str = "/tmp/reboot";

#[derive(Serialize)]
struct DetectResult {
    detected_card: Option<String>,
    detected_overlay: Option<String>,
}

fn read_config_lines(path: &str) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| content.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

/// Handle GET /api/v1/soundcard/detect - detect the connected HiFiBerry
/// sound card without persisting anything to config.txt.
pub async fn handle_detect_soundcard() -> impl IntoResponse {
    let config_lines = read_config_lines(CONFIG_TXT_PATH);
    let hat = read_hat_info(std::path::Path::new("/"));

    let mut detector = SoundcardDetector::new(config_lines, PathBuf::from(REBOOT_FILE_PATH));
    detector.detect_card(hat.product.as_deref());

    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "result": DetectResult {
                detected_card: detector.detected_card,
                detected_overlay: detector.detected_overlay,
            },
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    #[tokio::test]
    async fn handle_detect_soundcard_returns_success_envelope() {
        let response = handle_detect_soundcard().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["status"], "success");
        assert!(payload.get("result").is_some());
    }
}
