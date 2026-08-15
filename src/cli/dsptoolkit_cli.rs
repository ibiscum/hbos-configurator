//! CLI wrapper mirroring `configurator.dsptoolkit:main` (the
//! `config-dsptoolkit` tool).
use clap::Parser;

use crate::api::dsptoolkit::{DSPToolkit, ReqwestDspHttpClient, DEFAULT_DSP_HOST, DEFAULT_DSP_PORT, DEFAULT_TIMEOUT};

#[derive(Parser, Debug, PartialEq)]
#[command(name = "config-dsptoolkit", about = "HiFiBerry DSP Detection Tool")]
pub struct DsptoolkitArgs {
    /// DSP service hostname
    #[arg(long, default_value = DEFAULT_DSP_HOST)]
    pub host: String,

    /// DSP service port
    #[arg(long, default_value_t = DEFAULT_DSP_PORT)]
    pub port: u16,

    /// Request timeout in seconds
    #[arg(long, default_value_t = DEFAULT_TIMEOUT)]
    pub timeout: f64,

    /// Output results in JSON format
    #[arg(long)]
    pub json: bool,

    /// Output only the DSP name if detected
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Output only the detection status
    #[arg(long = "status-only")]
    pub status_only: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl Default for DsptoolkitArgs {
    fn default() -> Self {
        Self { host: DEFAULT_DSP_HOST.to_string(), port: DEFAULT_DSP_PORT, timeout: DEFAULT_TIMEOUT, json: false, name_only: false, status_only: false, verbose: false }
    }
}

/// Run the dsptoolkit CLI. Returns the process exit code (0 success, 1 failure).
pub fn run(args: &DsptoolkitArgs) -> i32 {
    let toolkit = DSPToolkit::new(&args.host, args.port, args.timeout);
    let client = ReqwestDspHttpClient;

    if args.name_only {
        return match toolkit.get_detected_dsp_name(&client) {
            Some(name) => {
                println!("{name}");
                0
            }
            None => 1,
        };
    }

    if args.status_only {
        let status = toolkit.get_dsp_status(&client);
        println!("{status}");
        return if status == "detected" { 0 } else { 1 };
    }

    if args.json {
        return match toolkit.detect_dsp(&client) {
            Some(info) => {
                println!("{}", serde_json::to_string_pretty(&info).unwrap());
                if info.get("status").and_then(|v| v.as_str()) == Some("detected") { 0 } else { 1 }
            }
            None => {
                println!("{{\n  \"status\": \"unavailable\"\n}}");
                1
            }
        };
    }

    match toolkit.detect_dsp(&client) {
        Some(info) => {
            let status = info.get("status").and_then(|v| v.as_str()).unwrap_or("error");
            if status == "detected" {
                let name = info.get("detected_dsp").and_then(|v| v.as_str()).unwrap_or("Unknown");
                println!("DSP detected: {name}");
                0
            } else {
                println!("DSP status: {status}");
                1
            }
        }
        None => {
            println!("DSP service unavailable");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = DsptoolkitArgs::parse_from(["config-dsptoolkit"]);
        assert_eq!(args, DsptoolkitArgs::default());
    }

    #[test]
    fn parses_all_options() {
        let args = DsptoolkitArgs::parse_from(["config-dsptoolkit", "--host", "dsp.local", "--port", "9999", "--timeout", "1.5", "--json", "--name-only", "--status-only", "--verbose"]);
        assert_eq!(args.host, "dsp.local");
        assert_eq!(args.port, 9999);
        assert_eq!(args.timeout, 1.5);
        assert!(args.json && args.name_only && args.status_only && args.verbose);
    }

    // No real DSP service runs in tests, so `run` is only exercised against
    // an unreachable loopback port (fast connection refusal, no network dependency).
    fn unreachable_args() -> DsptoolkitArgs {
        DsptoolkitArgs { host: "127.0.0.1".to_string(), port: 1, timeout: 0.5, ..DsptoolkitArgs::default() }
    }

    #[test]
    fn run_default_mode_returns_one_when_unavailable() {
        assert_eq!(run(&unreachable_args()), 1);
    }

    #[test]
    fn run_status_only_prints_unavailable_and_returns_one() {
        let args = DsptoolkitArgs { status_only: true, ..unreachable_args() };
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_name_only_returns_one_when_unavailable() {
        let args = DsptoolkitArgs { name_only: true, ..unreachable_args() };
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_json_mode_returns_one_when_unavailable() {
        let args = DsptoolkitArgs { json: true, ..unreachable_args() };
        assert_eq!(run(&args), 1);
    }
}
