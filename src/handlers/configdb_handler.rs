//! HTTP handler exposing `configurator/configdb.py`'s Flask handler methods
//! (`handle_get_config_keys`, `handle_get_config_value`,
//! `handle_set_config_value`, `handle_delete_config_value`).
use std::sync::{LazyLock, Mutex};

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::api::configdb::{parse_bool, ConfigDb};

/// Shared config database, opened once at first use.
pub static CONFIG_DB: LazyLock<Mutex<ConfigDb>> = LazyLock::new(|| Mutex::new(ConfigDb::open_default()));

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "status": "error", "message": message }))).into_response()
}

#[derive(Deserialize, Default)]
pub struct PrefixQuery {
    pub prefix: Option<String>,
}

/// Handle GET /api/v1/config/keys - list all configuration keys.
pub async fn handle_get_config_keys(Query(query): Query<PrefixQuery>) -> Response {
    let db = CONFIG_DB.lock().unwrap();
    let keys = db.list_keys(query.prefix.as_deref());
    Json(serde_json::json!({ "status": "success", "data": keys, "count": keys.len() })).into_response()
}

#[derive(Deserialize, Default)]
pub struct GetValueQuery {
    #[serde(default)]
    pub secure: String,
    pub default: Option<String>,
}

/// Handle GET /api/v1/config/key/:key - get a specific configuration value.
pub async fn handle_get_config_value(Path(key): Path<String>, Query(query): Query<GetValueQuery>) -> Response {
    let secure = if query.secure.is_empty() { false } else { parse_bool(&Value::String(query.secure.clone())).unwrap_or(false) };

    let db = CONFIG_DB.lock().unwrap();
    let value = db.get(&key, query.default.as_deref(), secure);

    match value {
        Some(value) => Json(serde_json::json!({ "status": "success", "data": { "key": key, "value": value } })).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("Configuration key \"{key}\" not found")),
    }
}

#[derive(Deserialize)]
pub struct SetValuePayload {
    value: Value,
    #[serde(default)]
    secure: Value,
}

/// Handle POST /api/v1/config/key/:key - set a configuration value.
pub async fn handle_set_config_value(Path(key): Path<String>, body: Option<Json<SetValuePayload>>) -> Response {
    let Some(Json(payload)) = body else {
        return error_response(StatusCode::BAD_REQUEST, "Malformed JSON body");
    };

    let secure = match parse_bool(&payload.secure) {
        Ok(s) => s,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Field \"secure\" must be a boolean"),
    };

    let value_str = match &payload.value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let mut db = CONFIG_DB.lock().unwrap();
    if db.set(&key, &value_str, secure) {
        Json(serde_json::json!({ "status": "success", "message": format!("Configuration key \"{key}\" set successfully"), "data": { "key": key, "value": value_str } })).into_response()
    } else {
        error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to set configuration value")
    }
}

/// Handle DELETE /api/v1/config/key/:key - delete a configuration value.
pub async fn handle_delete_config_value(Path(key): Path<String>) -> Response {
    let mut db = CONFIG_DB.lock().unwrap();
    if db.delete(&key) {
        Json(serde_json::json!({ "status": "success", "message": format!("Configuration key \"{key}\" deleted successfully") })).into_response()
    } else {
        error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete configuration value")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::Value as JsonValue;

    async fn body_json(response: Response) -> JsonValue {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn set_then_get_then_delete_roundtrip() {
        let key = "test.configdb.handler.key".to_string();

        let set_response = handle_set_config_value(Path(key.clone()), Some(Json(SetValuePayload { value: Value::String("hello".to_string()), secure: Value::Bool(false) }))).await;
        assert_eq!(set_response.status(), StatusCode::OK);
        assert_eq!(body_json(set_response).await["status"], "success");

        let get_response = handle_get_config_value(Path(key.clone()), Query(GetValueQuery::default())).await;
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(body_json(get_response).await["data"]["value"], "hello");

        let delete_response = handle_delete_config_value(Path(key.clone())).await;
        assert_eq!(delete_response.status(), StatusCode::OK);

        let missing_response = handle_get_config_value(Path(key), Query(GetValueQuery::default())).await;
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_missing_key_returns_not_found() {
        let response = handle_get_config_value(Path("no.such.key.at.all".to_string()), Query(GetValueQuery::default())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_rejects_invalid_secure_flag() {
        let response =
            handle_set_config_value(Path("test.configdb.invalid_secure".to_string()), Some(Json(SetValuePayload { value: Value::String("x".to_string()), secure: Value::from(2) }))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_keys_returns_success_envelope() {
        let response = handle_get_config_keys(Query(PrefixQuery::default())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await["status"], "success");
    }
}
