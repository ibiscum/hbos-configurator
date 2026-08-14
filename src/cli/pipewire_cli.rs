//! CLI wrapper mirroring `configurator.pipewire:main` (the `config-pipewire` tool).
use clap::{Parser, Subcommand};

use crate::api::pipewire::{self, SystemCommandRunner};

#[derive(Parser, Debug, PartialEq)]
#[command(name = "config-pipewire", about = "PipeWire volume control utility")]
pub struct PipewireArgs {
    #[command(subcommand)]
    pub command: Option<PipewireCommand>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum PipewireCommand {
    /// List all PipeWire volume control names
    List,
    /// Get the volume for a control (0.0-1.0)
    Get { control_name: String },
    /// Set the volume for a control (must be between 0.0 and 1.0)
    Set { control_name: String, volume: String },
}

fn print_usage() {
    println!("Usage:");
    println!("  config-pipewire list");
    println!("  config-pipewire get <control_name>");
    println!("  config-pipewire set <control_name> <volume>");
    println!("  (volume must be between 0.0 and 1.0)");
}

/// Run the pipewire CLI. Returns the process exit code (mirrors Python's `sys.exit`).
pub fn run(args: &PipewireArgs) -> i32 {
    let runner = SystemCommandRunner;

    match &args.command {
        None => {
            print_usage();
            1
        }
        Some(PipewireCommand::List) => {
            for control in pipewire::get_volume_controls(&runner) {
                println!("{control}");
            }
            0
        }
        Some(PipewireCommand::Get { control_name }) => match pipewire::get_volume(&runner, control_name) {
            Some(volume) => {
                println!("{volume}");
                0
            }
            None => {
                println!("Control '{control_name}' not found or no volume info.");
                2
            }
        },
        Some(PipewireCommand::Set { control_name, volume }) => {
            let Ok(volume) = volume.parse::<f64>() else {
                println!("Volume must be a float between 0.0 and 1.0");
                return 3;
            };
            if !(0.0..=1.0).contains(&volume) || !volume.is_finite() {
                println!("Volume must be a float between 0.0 and 1.0");
                return 3;
            }
            if pipewire::set_volume(&runner, control_name, volume) {
                println!("OK");
                0
            } else {
                println!("Failed to set volume for '{control_name}'");
                4
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_command() {
        let args = PipewireArgs::parse_from(["config-pipewire", "list"]);
        assert_eq!(args.command, Some(PipewireCommand::List));
    }

    #[test]
    fn parses_get_command_with_control_name() {
        let args = PipewireArgs::parse_from(["config-pipewire", "get", "Master"]);
        assert_eq!(args.command, Some(PipewireCommand::Get { control_name: "Master".to_string() }));
    }

    #[test]
    fn parses_set_command_with_control_and_volume() {
        let args = PipewireArgs::parse_from(["config-pipewire", "set", "Master", "0.5"]);
        assert_eq!(args.command, Some(PipewireCommand::Set { control_name: "Master".to_string(), volume: "0.5".to_string() }));
    }

    #[test]
    fn run_without_command_prints_usage_and_exits_1() {
        let args = PipewireArgs { command: None };
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_list_returns_zero() {
        let args = PipewireArgs::parse_from(["config-pipewire", "list"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_get_missing_control_exits_2() {
        let args = PipewireArgs::parse_from(["config-pipewire", "get", "Master"]);
        assert_eq!(run(&args), 2);
    }

    #[test]
    fn run_set_invalid_range_exits_3() {
        let args = PipewireArgs::parse_from(["config-pipewire", "set", "Master", "1.2"]);
        assert_eq!(run(&args), 3);
    }

    #[test]
    fn run_set_non_numeric_exits_3() {
        let args = PipewireArgs::parse_from(["config-pipewire", "set", "Master", "abc"]);
        assert_eq!(run(&args), 3);
    }
}
