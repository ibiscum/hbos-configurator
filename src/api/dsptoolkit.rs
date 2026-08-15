//! Port of `configurator/dsptoolkit.py` (DSP hardware detection via the
//! HiFiBerry DSP HTTP service).
use std::time::Duration;

use serde_json::{Map, Value};

pub const DEFAULT_DSP_HOST: &str = "localhost";
pub const DEFAULT_DSP_PORT: u16 = 13141;
pub const DEFAULT_TIMEOUT: f64 = 5.0;
pub const VALID_DSP_STATUSES: [&str; 4] = ["detected", "not_detected", "error", "unavailable"];

pub type DspInfo = Map<String, Value>;

/// Outcome of an HTTP GET against the DSP service (mirrors the exception
/// branches in the Python `requests.get` call).
#[derive(Debug, Clone, PartialEq)]
pub enum DspFetchResult {
    Success { status: u16, body: String },
    ConnectionError,
    Timeout,
    RequestError(String),
}

/// HTTP client abstraction for the DSP service (mirrors Python's `requests`
/// usage in `DSPToolkit.detect_dsp`).
pub trait DspHttpClient: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> DspFetchResult;
}

/// Real implementation backed by a blocking `reqwest` client.
pub struct ReqwestDspHttpClient;

impl DspHttpClient for ReqwestDspHttpClient {
    fn get(&self, url: &str, timeout: Duration) -> DspFetchResult {
        let client = match reqwest::blocking::Client::builder().timeout(timeout).build() {
            Ok(c) => c,
            Err(e) => return DspFetchResult::RequestError(e.to_string()),
        };
        match client.get(url).send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                match resp.text() {
                    Ok(body) => DspFetchResult::Success { status, body },
                    Err(e) => DspFetchResult::RequestError(e.to_string()),
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    DspFetchResult::Timeout
                } else if e.is_connect() {
                    DspFetchResult::ConnectionError
                } else {
                    DspFetchResult::RequestError(e.to_string())
                }
            }
        }
    }
}

/// Normalize an arbitrary status value to one of `VALID_DSP_STATUSES`,
/// falling back to `"error"`.
fn normalize_status(status: Option<&Value>) -> String {
    match status {
        Some(Value::String(s)) if VALID_DSP_STATUSES.contains(&s.as_str()) => s.clone(),
        _ => "error".to_string(),
    }
}

/// Normalize a DSP payload to always use a known status value, preserving
/// any other keys.
fn normalize_dsp_info(mut info: DspInfo) -> DspInfo {
    let status = normalize_status(info.get("status"));
    info.insert("status".to_string(), Value::String(status));
    info
}

fn error_info() -> DspInfo {
    let mut info = Map::new();
    info.insert("status".to_string(), Value::String("error".to_string()));
    info
}

/// Toolkit for DSP hardware detection and interaction.
pub struct DSPToolkit {
    pub host: String,
    pub port: u16,
    pub timeout: f64,
    pub base_url: String,
}

impl DSPToolkit {
    pub fn new(host: &str, port: u16, timeout: f64) -> Self {
        Self { host: host.to_string(), port, timeout, base_url: format!("http://{host}:{port}") }
    }

    /// Detect DSP hardware by querying the DSP service. Returns `None` when
    /// the service is unavailable or unreachable.
    pub fn detect_dsp(&self, client: &dyn DspHttpClient) -> Option<DspInfo> {
        let url = format!("{}/hardware/dsp", self.base_url);
        match client.get(&url, Duration::from_secs_f64(self.timeout)) {
            DspFetchResult::Success { status, body } => {
                if status == 200 {
                    match serde_json::from_str::<Value>(&body) {
                        Ok(Value::Object(map)) => Some(normalize_dsp_info(map)),
                        Ok(_) => {
                            tracing::error!("DSP detection response must be a JSON object");
                            Some(error_info())
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse DSP detection response as JSON: {e}");
                            Some(error_info())
                        }
                    }
                } else {
                    tracing::warn!("DSP service returned status code {status}");
                    if status >= 500 {
                        Some(error_info())
                    } else {
                        None
                    }
                }
            }
            DspFetchResult::ConnectionError => {
                tracing::debug!("DSP service not available (connection refused)");
                None
            }
            DspFetchResult::Timeout => {
                tracing::warn!("DSP service request timed out after {} seconds", self.timeout);
                None
            }
            DspFetchResult::RequestError(e) => {
                tracing::error!("Error communicating with DSP service: {e}");
                None
            }
        }
    }

    /// Get the name of the detected DSP, or `None` if not detected.
    pub fn get_detected_dsp_name(&self, client: &dyn DspHttpClient) -> Option<String> {
        let info = self.detect_dsp(client)?;
        if info.get("status").and_then(Value::as_str) != Some("detected") {
            return None;
        }
        info.get("detected_dsp").and_then(Value::as_str).map(str::to_string)
    }

    /// Check if a DSP is detected.
    pub fn is_dsp_detected(&self, client: &dyn DspHttpClient) -> bool {
        self.detect_dsp(client).map(|info| info.get("status").and_then(Value::as_str) == Some("detected")).unwrap_or(false)
    }

    /// Get the DSP detection status, defaulting to `"unavailable"` when the
    /// service cannot be reached.
    pub fn get_dsp_status(&self, client: &dyn DspHttpClient) -> String {
        match self.detect_dsp(client) {
            None => "unavailable".to_string(),
            Some(info) => normalize_status(info.get("status")),
        }
    }
}

impl Default for DSPToolkit {
    fn default() -> Self {
        Self::new(DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT)
    }
}

/// Detect DSP hardware (convenience function mirroring the Python module-level helper).
pub fn detect_dsp(client: &dyn DspHttpClient, host: &str, port: u16, timeout: f64) -> Option<DspInfo> {
    DSPToolkit::new(host, port, timeout).detect_dsp(client)
}

/// Get the name of the detected DSP (convenience function).
pub fn get_detected_dsp_name(client: &dyn DspHttpClient, host: &str, port: u16, timeout: f64) -> Option<String> {
    DSPToolkit::new(host, port, timeout).get_detected_dsp_name(client)
}

/// Check if a DSP is detected (convenience function).
pub fn is_dsp_detected(client: &dyn DspHttpClient, host: &str, port: u16, timeout: f64) -> bool {
    DSPToolkit::new(host, port, timeout).is_dsp_detected(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubClient(DspFetchResult);

    impl DspHttpClient for StubClient {
        fn get(&self, _url: &str, _timeout: Duration) -> DspFetchResult {
            self.0.clone()
        }
    }

    fn toolkit() -> DSPToolkit {
        DSPToolkit::default()
    }

    #[test]
    fn default_constants_match_python() {
        assert_eq!(DEFAULT_DSP_HOST, "localhost");
        assert_eq!(DEFAULT_DSP_PORT, 13141);
        assert_eq!(DEFAULT_TIMEOUT, 5.0);
    }

    #[test]
    fn base_url_is_built_from_host_and_port() {
        let t = DSPToolkit::new("example.local", 9999, 1.0);
        assert_eq!(t.base_url, "http://example.local:9999");
    }

    #[test]
    fn detect_dsp_success_normalizes_and_preserves_extra_keys() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"detected_dsp":"ADAU14xx","status":"detected","extra":1}"#.to_string() });
        let info = toolkit().detect_dsp(&client).unwrap();
        assert_eq!(info.get("status").unwrap(), "detected");
        assert_eq!(info.get("detected_dsp").unwrap(), "ADAU14xx");
        assert_eq!(info.get("extra").unwrap(), 1);
    }

    #[test]
    fn detect_dsp_success_with_invalid_status_becomes_error() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"status":"bogus"}"#.to_string() });
        let info = toolkit().detect_dsp(&client).unwrap();
        assert_eq!(info.get("status").unwrap(), "error");
    }

    #[test]
    fn detect_dsp_success_non_object_json_returns_error_status() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: "[1,2,3]".to_string() });
        let info = toolkit().detect_dsp(&client).unwrap();
        assert_eq!(info.get("status").unwrap(), "error");
    }

    #[test]
    fn detect_dsp_success_invalid_json_returns_error_status() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: "not json".to_string() });
        let info = toolkit().detect_dsp(&client).unwrap();
        assert_eq!(info.get("status").unwrap(), "error");
    }

    #[test]
    fn detect_dsp_server_error_status_code_returns_error() {
        let client = StubClient(DspFetchResult::Success { status: 503, body: String::new() });
        let info = toolkit().detect_dsp(&client).unwrap();
        assert_eq!(info.get("status").unwrap(), "error");
    }

    #[test]
    fn detect_dsp_client_error_status_code_returns_none() {
        let client = StubClient(DspFetchResult::Success { status: 404, body: String::new() });
        assert_eq!(toolkit().detect_dsp(&client), None);
    }

    #[test]
    fn detect_dsp_connection_error_returns_none() {
        let client = StubClient(DspFetchResult::ConnectionError);
        assert_eq!(toolkit().detect_dsp(&client), None);
    }

    #[test]
    fn detect_dsp_timeout_returns_none() {
        let client = StubClient(DspFetchResult::Timeout);
        assert_eq!(toolkit().detect_dsp(&client), None);
    }

    #[test]
    fn detect_dsp_request_error_returns_none() {
        let client = StubClient(DspFetchResult::RequestError("boom".to_string()));
        assert_eq!(toolkit().detect_dsp(&client), None);
    }

    #[test]
    fn get_detected_dsp_name_returns_name_when_detected() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"detected_dsp":"ADAU14xx","status":"detected"}"#.to_string() });
        assert_eq!(toolkit().get_detected_dsp_name(&client), Some("ADAU14xx".to_string()));
    }

    #[test]
    fn get_detected_dsp_name_none_when_not_detected() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"status":"not_detected"}"#.to_string() });
        assert_eq!(toolkit().get_detected_dsp_name(&client), None);
    }

    #[test]
    fn get_detected_dsp_name_none_when_service_unavailable() {
        let client = StubClient(DspFetchResult::ConnectionError);
        assert_eq!(toolkit().get_detected_dsp_name(&client), None);
    }

    #[test]
    fn is_dsp_detected_true_when_status_detected() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"status":"detected"}"#.to_string() });
        assert!(toolkit().is_dsp_detected(&client));
    }

    #[test]
    fn is_dsp_detected_false_when_unavailable() {
        let client = StubClient(DspFetchResult::Timeout);
        assert!(!toolkit().is_dsp_detected(&client));
    }

    #[test]
    fn get_dsp_status_unavailable_when_none() {
        let client = StubClient(DspFetchResult::ConnectionError);
        assert_eq!(toolkit().get_dsp_status(&client), "unavailable");
    }

    #[test]
    fn get_dsp_status_returns_normalized_status() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"status":"not_detected"}"#.to_string() });
        assert_eq!(toolkit().get_dsp_status(&client), "not_detected");
    }

    #[test]
    fn convenience_functions_match_toolkit_methods() {
        let client = StubClient(DspFetchResult::Success { status: 200, body: r#"{"detected_dsp":"ADAU14xx","status":"detected"}"#.to_string() });
        assert_eq!(get_detected_dsp_name(&client, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT), Some("ADAU14xx".to_string()));
        assert!(is_dsp_detected(&client, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT));
        assert!(detect_dsp(&client, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT).is_some());
    }

    #[test]
    fn reqwest_client_reports_connection_error_for_unreachable_loopback_port() {
        let client = ReqwestDspHttpClient;
        // Port 1 on loopback is reserved/unlikely to be listening; a fast
        // connection refusal (not a real network call) is expected.
        let result = client.get("http://127.0.0.1:1/hardware/dsp", Duration::from_millis(500));
        assert_eq!(result, DspFetchResult::ConnectionError);
    }
}
