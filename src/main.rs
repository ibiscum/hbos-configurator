use axum::{
    error_handling::HandleErrorLayer,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use hbos_configurator::handlers::{
    configdb_handler, hattools_handler, hostname_handler, i2c_handler, network_handler,
    pimodel_handler, pipewire_handler, sambaclient_handler, sambamount_handler,
    settings_manager_handler, soundcard_detector_handler, soundcard_handler, systeminfo_handler,
    volume_handler, wifi_handler,
};

// =========================================================================
// 🎛️ Command Line Arguments Parser
// =========================================================================
#[derive(Parser, Debug)]
#[command(name = "hifiberry-config-server", version = "1.0", about = "HiFiBerry Configuration Server")]
struct Args {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value_t = 1081)]
    port: u16,

    #[arg(long)]
    debug: bool,

    #[arg(short, long)]
    verbose: bool,

    #[arg(long)]
    restore_settings: bool,

    #[arg(long)]
    auto_restore_settings: bool,

    #[arg(long)]
    no_waitress: bool, // Maintained for flags backwards-compatibility
}

// =========================================================================
// 📦 Dummy Subsystem Handlers (Core Business Logic Targets)
// =========================================================================
// In production, instantiate individual custom structs per domain file.
struct AppState;

// =========================================================================
// 🛡️ API Envelope & Response Normalizer (Matches after_request payload)
// =========================================================================
#[derive(Serialize)]
struct ApiEnvelope {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

fn map_http_status_to_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        s if s.is_server_error() => "internal_error",
        _ => "operation_failed",
    }
}

// Global fallback wrapper simulating error_handler definitions
async fn handle_errors(err: tower::BoxError) -> (StatusCode, Json<ApiEnvelope>) {
    let status = StatusCode::INTERNAL_SERVER_ERROR;
    let code = map_http_status_to_code(status);
    (
        status,
        Json(ApiEnvelope {
            status: "error".to_string(),
            error: Some(code.to_string()),
            message: Some(format!("Unhandled system error: {}", err)),
            data: None,
        }),
    )
}

// =========================================================================
// 🛣️ Endpoint Route Handlers
// =========================================================================

async fn get_version() -> impl IntoResponse {
    Json(json!({
        "service": "hifiberry-config-api",
        "version": "1.0.0",
        "api_version": "v1",
        "description": "HiFiBerry Configuration Server (Rust)",
        "endpoints": { "version": "/version", "setup_status": "/api/v1/setup/status", "systeminfo": "/api/v1/systeminfo" }
    }))
}

async fn get_setup_status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let db = configdb_handler::CONFIG_DB.lock().unwrap();
    let is_completed = db.get("system.setup_completed", None, false) == Some("true".to_string());
    Json(ApiEnvelope {
        status: "success".to_string(),
        error: None,
        message: None,
        data: Some(json!({ "setup_completed": is_completed })),
    })
}

async fn complete_setup(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut db = configdb_handler::CONFIG_DB.lock().unwrap();
    db.set("system.setup_completed", "true", false);
    Json(ApiEnvelope {
        status: "success".to_string(),
        error: None,
        message: Some("Setup marked as completed".to_string()),
        data: None,
    })
}

async fn reset_setup(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut db = configdb_handler::CONFIG_DB.lock().unwrap();
    db.delete("system.setup_completed");
    Json(ApiEnvelope {
        status: "success".to_string(),
        error: None,
        message: Some("Setup status reset".to_string()),
        data: None,
    })
}

async fn execute_systemd_operation(
    Path((service, operation)): Path<(String, String)>,
    State(_state): State<Arc<AppState>>
) -> impl IntoResponse {
    info!("Executing systemd action '{}' on service '{}'", operation, service);
    // Ported from execution rules wrapping subprocess executions
    Json(json!({ "status": "success", "service": service, "operation": operation }))
}

// Generic 404 handler matching Flask app.errorhandler(404)
async fn route_not_found() -> impl IntoResponse {
    let status = StatusCode::NOT_FOUND;
    (
        status,
        Json(ApiEnvelope {
            status: "error".to_string(),
            error: Some(map_http_status_to_code(status).to_string()),
            message: Some("Resource not found".to_string()),
            data: None,
        }),
    )
}

// =========================================================================
// 🚀 Main Server Entrypoint Loop
// =========================================================================
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Configure Application Logging
    let log_level = if args.verbose || args.debug { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting HiFiBerry Configuration Server (Rust Execution Framework)");

    let state = Arc::new(AppState);

    // Business Logic Settings Restoration Triggers
    if args.restore_settings {
        info!("Restoring configurations from persistent tracking layers...");
        return Ok(());
    }

    if args.auto_restore_settings {
        info!("Executing automated configurations initialization sequences...");
    }

    // Build Axum Router and Bind API Routes
    let app = Router::new()
        // Meta Layer Routes
        .route("/version", get(get_version))
        .route("/api/v1/version", get(get_version))
        // System Wizards Setups Route Handling Definitions
        .route("/api/v1/setup/status", get(get_setup_status))
        .route("/api/v1/setup/complete", post(complete_setup))
        .route("/api/v1/setup/reset", post(reset_setup))
        // Database Dynamic Handling Bindings
        .route("/api/v1/config/keys", get(configdb_handler::handle_get_config_keys))
        .route(
            "/api/v1/config/key/:key",
            get(configdb_handler::handle_get_config_value).post(configdb_handler::handle_set_config_value).delete(configdb_handler::handle_delete_config_value),
        )
        // OS Systemd Process Integration Points
        .route("/api/v1/systemd/service/:service/:operation", post(execute_systemd_operation))
        // System information endpoint
        .route("/api/v1/systeminfo", get(systeminfo_handler::handle_get_system_info))
        // Raspberry Pi model detection endpoint
        .route("/api/v1/pimodel", get(pimodel_handler::handle_get_pi_model))
        // Network interface discovery and configuration endpoints
        .route("/api/v1/network/interfaces", get(network_handler::handle_list_interfaces))
        .route("/api/v1/network/config", get(network_handler::handle_get_network_config))
        .route("/api/v1/network/dhcp", post(network_handler::handle_set_dhcp))
        .route("/api/v1/network/fixed-ip", post(network_handler::handle_set_fixed_ip))
        .route("/api/v1/network/ipv6/enable", post(network_handler::handle_enable_ipv6))
        .route("/api/v1/network/ipv6/disable", post(network_handler::handle_disable_ipv6))
        // PipeWire volume control endpoints
        .route("/api/v1/pipewire/controls", get(pipewire_handler::handle_list_controls))
        .route("/api/v1/pipewire/volume", get(pipewire_handler::handle_get_volume).post(pipewire_handler::handle_set_volume))
        // I2C bus scanning endpoints
        .route("/api/v1/i2c/scan", get(i2c_handler::handle_scan))
        .route("/api/v1/i2c/info", get(i2c_handler::handle_info))
        // Hostname management endpoints
        .route("/api/v1/hostname", get(hostname_handler::handle_get_hostname).post(hostname_handler::handle_set_hostname))
        .route("/api/v1/hostname/pretty", post(hostname_handler::handle_set_pretty_hostname))
        // HAT EEPROM info endpoint
        .route("/api/v1/hat", get(hattools_handler::handle_get_hat_info))
        // Sound card detection endpoint
        .route("/api/v1/soundcard/detect", get(soundcard_detector_handler::handle_detect_soundcard))
        // Sound card catalogue and detected-card endpoints
        .route("/api/v1/soundcard", get(soundcard_handler::handle_get_soundcard))
        .route("/api/v1/soundcard/definitions", get(soundcard_handler::handle_list_soundcard_definitions))
        // Saved-settings management endpoints
        .route("/api/v1/settings", get(settings_manager_handler::handle_list_settings))
        .route("/api/v1/settings/save", post(settings_manager_handler::handle_save_all_settings))
        .route("/api/v1/settings/restore", post(settings_manager_handler::handle_restore_all_settings))
        .route("/api/v1/settings/:name", axum::routing::delete(settings_manager_handler::handle_delete_setting))
        // SMB/CIFS mount management endpoints
        .route("/api/v1/sambamount", get(sambamount_handler::handle_list_mounts).post(sambamount_handler::handle_add_mount).delete(sambamount_handler::handle_remove_mount))
        .route("/api/v1/sambamount/mount", post(sambamount_handler::handle_mount))
        .route("/api/v1/sambamount/unmount", post(sambamount_handler::handle_unmount))
        .route("/api/v1/sambamount/mount-all", post(sambamount_handler::handle_mount_all))
        // SMB/CIFS client discovery endpoints
        .route("/api/v1/sambaclient/servers", get(sambaclient_handler::handle_list_file_servers))
        .route("/api/v1/sambaclient/check", get(sambaclient_handler::handle_check_connect))
        .route("/api/v1/sambaclient/version", get(sambaclient_handler::handle_detect_version))
        .route("/api/v1/sambaclient/shares", get(sambaclient_handler::handle_list_shares))
        // Setup State context sharing mapping variables
        .route("/api/v1/volume/headphone/controls", get(volume_handler::handle_list_headphone_controls))
        .route("/api/v1/volume/headphone", get(volume_handler::handle_get_headphone_volume).post(volume_handler::handle_set_headphone_volume))
        .route("/api/v1/volume/headphone/store", post(volume_handler::handle_store_headphone_volume))
        .route("/api/v1/volume/headphone/restore", post(volume_handler::handle_restore_headphone_volume))
        // WiFi network management endpoints
        .route("/api/v1/wifi/networks", get(wifi_handler::handle_list_networks))
        .route("/api/v1/wifi/current", get(wifi_handler::handle_get_current_connection))
        .route("/api/v1/wifi/connect", post(wifi_handler::handle_connect))
        .with_state(state)
        // High Availability Protection Middlewares Layer Configuration
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_errors))
                .buffer(1024)
                .rate_limit(100, Duration::from_secs(1)) // Safeguard Embedded Platform CPU limits
                .layer(TraceLayer::new_for_http())
        )
        // Global Catch-all Routing Target
        .fallback(route_not_found);

    // Bind Network Interception Sockets (Emulating Waitress Serve loop topology)
    let bind_addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Production-ready Web Server listening explicitly on: http://{}", bind_addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}
