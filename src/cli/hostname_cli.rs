//! CLI wrapper mirroring `configurator.hostconfig:main` (the `config-hostname`
//! tool), extended with `set-pretty` from `configurator.hostname_utils`.
use std::path::Path;

use clap::{Parser, Subcommand};

use crate::api::hostconfig::{self, SystemCommandRunner};
use crate::api::hostname_utils;

#[derive(Parser, Debug, PartialEq)]
#[command(name = "config-hostname", about = "Manage system hostname configuration")]
pub struct HostnameArgs {
    #[command(subcommand)]
    pub command: Option<HostnameCommand>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum HostnameCommand {
    /// Get current system hostname
    Get,
    /// Validate hostname format
    Validate { hostname: String },
    /// Convert pretty hostname to valid format
    Sanitize {
        hostname: String,
        #[arg(long, default_value_t = 64)]
        max_length: i32,
    },
    /// Set system hostname
    Set { hostname: String },
    /// Set pretty (display) hostname
    SetPretty { pretty_hostname: String },
}

/// Run the hostname CLI. Returns the process exit code (mirrors Python's `main()`).
pub fn run(args: &HostnameArgs) -> i32 {
    let runner = SystemCommandRunner;

    match &args.command {
        None => 0,
        Some(HostnameCommand::Get) => match hostconfig::get_current_hostname(&runner) {
            Some(hostname) => {
                println!("{hostname}");
                0
            }
            None => {
                eprintln!("Failed to get hostname");
                1
            }
        },
        Some(HostnameCommand::Validate { hostname }) => {
            if hostconfig::validate_hostname(hostname) {
                println!("'{hostname}' is a valid hostname");
                0
            } else {
                eprintln!("'{hostname}' is not a valid hostname");
                1
            }
        }
        Some(HostnameCommand::Sanitize { hostname, max_length }) => {
            println!("{}", hostconfig::sanitize_hostname(hostname, *max_length));
            0
        }
        Some(HostnameCommand::Set { hostname }) => {
            if !hostconfig::validate_hostname(hostname) {
                eprintln!("'{hostname}' is not a valid hostname");
                return 1;
            }
            if hostconfig::set_hostname_with_hosts_update(&runner, Path::new("/"), hostname) {
                println!("Successfully set hostname to '{hostname}'");
                0
            } else {
                eprintln!("Failed to set hostname");
                1
            }
        }
        Some(HostnameCommand::SetPretty { pretty_hostname }) => {
            if !hostname_utils::validate_pretty_hostname(pretty_hostname) {
                eprintln!("'{pretty_hostname}' is not a valid pretty hostname");
                return 1;
            }
            if hostname_utils::set_pretty_hostname(&runner, pretty_hostname) {
                println!("Successfully set pretty hostname to '{pretty_hostname}'");
                0
            } else {
                eprintln!("Failed to set pretty hostname");
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_command() {
        let args = HostnameArgs::parse_from(["config-hostname", "get"]);
        assert_eq!(args.command, Some(HostnameCommand::Get));
    }

    #[test]
    fn parses_sanitize_with_max_length() {
        let args = HostnameArgs::parse_from(["config-hostname", "sanitize", "My Host", "--max-length", "10"]);
        assert_eq!(args.command, Some(HostnameCommand::Sanitize { hostname: "My Host".to_string(), max_length: 10 }));
    }

    #[test]
    fn run_validate_valid_hostname_returns_zero() {
        let args = HostnameArgs::parse_from(["config-hostname", "validate", "validhost"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_validate_invalid_hostname_returns_one() {
        let args = HostnameArgs::parse_from(["config-hostname", "validate", "hostname-"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_sanitize_command_produces_valid_hostname() {
        let args = HostnameArgs::parse_from(["config-hostname", "sanitize", "My-Host!"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_set_invalid_hostname_returns_one() {
        let args = HostnameArgs::parse_from(["config-hostname", "set", "hostname-"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_no_command_returns_zero() {
        let args = HostnameArgs { command: None };
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_set_pretty_rejects_non_printable() {
        let args = HostnameArgs::parse_from(["config-hostname", "set-pretty", "HiFi\nBerry"]);
        assert_eq!(run(&args), 1);
    }
}
