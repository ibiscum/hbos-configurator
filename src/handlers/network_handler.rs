//! HTTP handler exposing `configurator/network.py` functionality (no Python
//! `handlers/` equivalent exists; endpoints follow the same conventions as
//! the other handlers).
use std::path::Path;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::api::network::{self, SystemCommandRunner, SystemNetworkInterfaces};

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

fn is_physical(runner: &SystemCommandRunner, interface: &str) -> bool {
    network::is_physical_interface(interface, runner, Path::new("/"))
}

/// Handle GET /api/v1/network/interfaces - list physical network interfaces.
pub async fn handle_list_interfaces() -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let nics = SystemNetworkInterfaces { runner: &runner };
    let interfaces = network::list_physical_interfaces(&nics, &|iface| is_physical(&runner, iface), Path::new("/"));

    (StatusCode::OK, Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(interfaces), error: None }))
}

/// Handle GET /api/v1/network/config - hostname, gateway, DNS and interfaces.
pub async fn handle_get_network_config() -> impl IntoResponse {
    let runner = SystemCommandRunner;
    let nics = SystemNetworkInterfaces { runner: &runner };
    let config = network::get_network_config(&nics, &|iface| is_physical(&runner, iface), &runner, Path::new("/"));

    (StatusCode::OK, Json(GenericResponse { status: ApiResponseStatus::Success, message: None, data: Some(config), error: None }))
}

#[derive(Deserialize)]
pub struct DhcpPayload {
    interface: String,
}

/// Handle POST /api/v1/network/dhcp - configure an interface to use DHCP.
pub async fn handle_set_dhcp(Json(payload): Json<DhcpPayload>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    if network::configure_dhcp(&runner, &payload.interface, &|iface| is_physical(&runner, iface)) {
        Json(GenericResponse::<()> { status: ApiResponseStatus::Success, message: Some(format!("Interface {} configured to use DHCP", payload.interface)), data: None, error: None })
            .into_response()
    } else {
        error_response(&format!("Failed to configure DHCP on interface {}", payload.interface), "dhcp_configuration_failed", StatusCode::BAD_REQUEST)
    }
}

#[derive(Deserialize)]
pub struct FixedIpPayload {
    interface: String,
    ip: String,
    router: String,
}

/// Handle POST /api/v1/network/fixed-ip - configure an interface with a static IP.
pub async fn handle_set_fixed_ip(Json(payload): Json<FixedIpPayload>) -> axum::response::Response {
    let runner = SystemCommandRunner;
    if network::configure_fixed_ip(&runner, &payload.interface, &payload.ip, &payload.router, &|iface| is_physical(&runner, iface)) {
        Json(GenericResponse::<()> {
            status: ApiResponseStatus::Success,
            message: Some(format!("Interface {} configured with static IP {}", payload.interface, payload.ip)),
            data: None,
            error: None,
        })
        .into_response()
    } else {
        error_response(&format!("Failed to configure static IP on interface {}", payload.interface), "static_ip_configuration_failed", StatusCode::BAD_REQUEST)
    }
}

/// Handle POST /api/v1/network/ipv6/enable - enable IPv6 system-wide.
pub async fn handle_enable_ipv6() -> axum::response::Response {
    let runner = SystemCommandRunner;
    if network::enable_ipv6(&runner, Path::new("/etc/sysctl.d")) {
        Json(GenericResponse::<()> { status: ApiResponseStatus::Success, message: Some("IPv6 enabled system-wide".to_string()), data: None, error: None }).into_response()
    } else {
        error_response("Failed to enable IPv6 system-wide", "ipv6_enable_failed", StatusCode::BAD_REQUEST)
    }
}

/// Handle POST /api/v1/network/ipv6/disable - disable IPv6 system-wide.
pub async fn handle_disable_ipv6() -> axum::response::Response {
    let runner = SystemCommandRunner;
    if network::disable_ipv6(&runner, Path::new("/etc/sysctl.d")) {
        Json(GenericResponse::<()> { status: ApiResponseStatus::Success, message: Some("IPv6 disabled system-wide".to_string()), data: None, error: None }).into_response()
    } else {
        error_response("Failed to disable IPv6 system-wide", "ipv6_disable_failed", StatusCode::BAD_REQUEST)
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
    async fn list_interfaces_returns_success_envelope() {
        let response = handle_list_interfaces().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"].is_array());
    }

    #[tokio::test]
    async fn get_network_config_returns_success_envelope() {
        let response = handle_get_network_config().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let payload = body_json(response).await;
        assert_eq!(payload["status"], "success");
        assert!(payload["data"]["hostname"].is_string());
    }

    #[tokio::test]
    async fn set_dhcp_reports_failure_without_network_manager() {
        let response = handle_set_dhcp(Json(DhcpPayload { interface: "eth0".to_string() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_fixed_ip_reports_failure_without_network_manager() {
        let response = handle_set_fixed_ip(Json(FixedIpPayload { interface: "eth0".to_string(), ip: "192.168.1.10/24".to_string(), router: "192.168.1.1".to_string() })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
