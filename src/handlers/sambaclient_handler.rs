//! HTTP handler exposing `configurator/sambaclient.py` functionality (no
//! Python `handlers/` equivalent exists; endpoints follow the same
//! conventions as the other handlers).
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::api::sambaclient::{self, SystemCommandRunner, SystemNetworkInterfaces};

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

#[derive(Deserialize, Default)]
pub struct AuthQuery {
    pub user: Option<String>,
    pub password: Option<String>,
    pub credentials: Option<String>,
}

#[derive(Deserialize)]
pub struct ServerQuery {
    pub server: String,
    #[serde(flatten)]
    pub auth: AuthQuery,
}

#[derive(Deserialize)]
pub struct ListSharesQuery {
    pub server: String,
    #[serde(flatten)]
    pub auth: AuthQuery,
    pub smbversion: Option<String>,
}

/// Handle GET /api/v1/sambaclient/servers - list SMB file servers on the local network.
pub async fn handle_list_file_servers() -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let nics = SystemNetworkInterfaces { runner: &runner };
    let servers: Vec<_> = sambaclient::list_all_servers(&runner, &nics).into_iter().filter(|s| s.is_file_server).collect();

    (StatusCode::OK, Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(servers), error: None }))
}

/// Handle GET /api/v1/sambaclient/check?server=... - test a connection to an SMB server.
pub async fn handle_check_connect(Query(query): Query<ServerQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    match sambaclient::check_smb_connection(&runner, &query.server, query.auth.user.as_deref(), query.auth.password.as_deref(), query.auth.credentials.as_deref()) {
        Ok(()) => Json(GenericResponse::<()> { status: ApiResponseStatus::Success, message: Some("Connection successful".to_string()), data: None, error: None }).into_response(),
        Err(msg) => error_response(&msg, "connection_failed", StatusCode::BAD_REQUEST),
    }
}

/// Handle GET /api/v1/sambaclient/version?server=... - detect the SMB version supported by a server.
pub async fn handle_detect_version(Query(query): Query<ServerQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    let version = sambaclient::detect_smb_version(&runner, &query.server, query.auth.user.as_deref(), query.auth.password.as_deref(), query.auth.credentials.as_deref());

    if version == "Unknown" {
        return error_response(&format!("Could not detect SMB version for {}", query.server), "version_detection_failed", StatusCode::BAD_REQUEST);
    }
    Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(serde_json::json!({ "version": version })), error: None }).into_response()
}

/// Handle GET /api/v1/sambaclient/shares?server=... - list shares on an SMB server.
pub async fn handle_list_shares(Query(query): Query<ListSharesQuery>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    let (shares, detected_version) =
        sambaclient::list_smb_shares(&runner, &query.server, query.auth.user.as_deref(), query.auth.password.as_deref(), query.auth.credentials.as_deref(), query.smbversion.as_deref());

    if shares.is_empty() {
        return error_response(&format!("No accessible shares found on {}", query.server), "no_shares_found", StatusCode::NOT_FOUND);
    }
    Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(serde_json::json!({ "shares": shares, "smb_version": detected_version })), error: None })
        .into_response()
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
    async fn list_file_servers_returns_success_envelope() {
        let response = handle_list_file_servers().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"].is_array());
    }

    #[tokio::test]
    async fn check_connect_rejects_option_like_server() {
        let response = handle_check_connect(Query(ServerQuery { server: "--help".to_string(), auth: AuthQuery::default() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(response).await["status"], "error");
    }

    #[tokio::test]
    async fn detect_version_reports_failure_without_reachable_server() {
        let response = handle_detect_version(Query(ServerQuery { server: "unreachable.invalid".to_string(), auth: AuthQuery::default() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_shares_reports_not_found_without_reachable_server() {
        let response = handle_list_shares(Query(ListSharesQuery { server: "unreachable.invalid".to_string(), auth: AuthQuery::default(), smbversion: None })).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
