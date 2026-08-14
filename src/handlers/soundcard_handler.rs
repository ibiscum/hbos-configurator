//! HTTP handler exposing `configurator/soundcard.py` functionality for the
//! `/api/v1/soundcard` endpoints.
use std::fs;

use axum::{
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::api::soundcard::{Soundcard, SOUND_CARD_DEFINITIONS};
use crate::api::soundcard_detector::SystemCommandRunner;
use crate::api::systeminfo::read_hat_info;

const CONFIG_TXT_PATH: &str = "/boot/firmware/config.txt";

fn read_config_lines(path: &str) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| content.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

fn detect(no_eeprom: bool) -> Soundcard {
    let config_lines = read_config_lines(CONFIG_TXT_PATH);
    let hat = read_hat_info(std::path::Path::new("/"));
    let runner = SystemCommandRunner;
    Soundcard::detect(no_eeprom, &config_lines, hat.product.as_deref(), &runner)
}

#[derive(Deserialize, Default)]
pub struct DetectQuery {
    #[serde(default)]
    pub no_eeprom: bool,
}

/// Handle GET /api/v1/soundcard - detect the connected HiFiBerry sound card.
pub async fn handle_get_soundcard(Query(query): Query<DetectQuery>) -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let card = detect(query.no_eeprom);
    let hardware_index = card.get_hardware_index(&runner);

    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "soundcard": {
                "name": card.name,
                "volume_control": card.volume_control,
                "headphone_volume_control": card.headphone_volume_control,
                "hardware_index": hardware_index,
                "output_channels": card.output_channels,
                "input_channels": card.input_channels,
                "features": card.features,
                "hat_name": card.hat_name,
                "supports_dsp": card.supports_dsp,
                "card_type": card.card_type,
            },
        })),
    )
}

/// Handle GET /api/v1/soundcard/definitions - list the full sound card catalogue.
pub async fn handle_list_soundcard_definitions() -> impl IntoResponse {
    let definitions: Vec<_> = SOUND_CARD_DEFINITIONS
        .iter()
        .map(|(name, def)| json!({ "name": name, "definition": def }))
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "count": definitions.len(),
            "definitions": definitions,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value;

    #[tokio::test]
    async fn handle_get_soundcard_returns_success_envelope() {
        let response = handle_get_soundcard(Query(DetectQuery::default())).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["status"], "success");
        assert!(payload.get("soundcard").is_some());
        assert!(payload["soundcard"].get("name").is_some());
    }

    #[tokio::test]
    async fn handle_list_soundcard_definitions_returns_full_catalogue() {
        let response = handle_list_soundcard_definitions().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["status"], "success");
        assert_eq!(payload["count"], SOUND_CARD_DEFINITIONS.len());
    }
}
