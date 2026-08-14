//! CLI wrapper mirroring `configurator.wifi:main` (the `config-wifi` tool).
use clap::Parser;

use crate::api::wifi::{self, SystemCommandRunner};

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-wifi", about = "WiFi Network Management Tool")]
pub struct WifiArgs {
    /// List available WiFi networks
    #[arg(long)]
    pub list_networks: bool,

    /// Connect to specified WiFi network
    #[arg(long, value_name = "SSID")]
    pub connect: Option<String>,

    /// Show currently connected WiFi network
    #[arg(long)]
    pub show_current: bool,

    /// Maximum scanning time in seconds
    #[arg(long, default_value_t = 10)]
    pub timeout: u64,

    /// Passphrase for the WiFi network
    #[arg(long)]
    pub passphrase: Option<String>,

    /// Revert to previous connection if new connection fails
    #[arg(long)]
    pub revert_when_fail: bool,

    /// Display detailed network information
    #[arg(long)]
    pub long: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress all output except warnings and errors
    #[arg(short, long)]
    pub quiet: bool,
}

/// Run the wifi CLI. Returns the process exit code (mirrors Python's `main() -> int`).
pub fn run(args: &WifiArgs) -> i32 {
    let selected = [args.list_networks, args.connect.is_some(), args.show_current]
        .iter()
        .filter(|s| **s)
        .count();
    if selected != 1 {
        eprintln!("Exactly one of --list-networks, --connect, --show-current is required");
        return 1;
    }

    let runner = SystemCommandRunner;

    if args.list_networks {
        let networks = wifi::scan_wifi_networks(&runner);
        if networks.is_empty() {
            eprintln!("No WiFi networks found");
            return 1;
        }
        if !args.quiet {
            for n in &networks {
                if args.long {
                    println!("{}|{}|{}|{}|{}", n.ssid, n.signal, n.security, n.channel, n.bssid);
                } else {
                    println!("{}|{}|{}", n.ssid, n.signal, n.security);
                }
            }
        }
        return 0;
    }

    if let Some(ssid) = &args.connect {
        return if wifi::connect_to_wifi(&runner, ssid, args.passphrase.as_deref(), args.revert_when_fail) {
            0
        } else {
            1
        };
    }

    if args.show_current {
        return match wifi::get_current_connection(&runner) {
            Some(conn) => {
                if !args.quiet {
                    if args.long {
                        println!(
                            "{}|{}|{}|{}",
                            conn.ssid,
                            conn.device,
                            conn.ip.as_deref().unwrap_or("Unknown"),
                            conn.security
                        );
                    } else {
                        println!("{}|{}", conn.ssid, conn.ip.as_deref().unwrap_or("Unknown"));
                    }
                }
                0
            }
            None => {
                eprintln!("Not currently connected to any WiFi network");
                1
            }
        };
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_networks_flag() {
        let args = WifiArgs::parse_from(["config-wifi", "--list-networks"]);
        assert!(args.list_networks);
        assert_eq!(args.timeout, 10);
    }

    #[test]
    fn parses_connect_with_passphrase_and_revert() {
        let args = WifiArgs::parse_from([
            "config-wifi",
            "--connect",
            "MyNet",
            "--passphrase",
            "secret",
            "--revert-when-fail",
        ]);
        assert_eq!(args.connect.as_deref(), Some("MyNet"));
        assert_eq!(args.passphrase.as_deref(), Some("secret"));
        assert!(args.revert_when_fail);
    }

    #[test]
    fn parses_show_current_and_long() {
        let args = WifiArgs::parse_from(["config-wifi", "--show-current", "--long"]);
        assert!(args.show_current);
        assert!(args.long);
    }

    #[test]
    fn run_requires_exactly_one_command() {
        let args = WifiArgs::default();
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_rejects_multiple_commands() {
        let mut args = WifiArgs::default();
        args.list_networks = true;
        args.show_current = true;
        assert_eq!(run(&args), 1);
    }
}
