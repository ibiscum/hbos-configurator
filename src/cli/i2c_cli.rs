//! CLI wrapper around `configurator/i2c.py` functionality (the Python module
//! has no standalone `main()`; this exposes the same operations as the
//! `config-i2c` tool for scripting/debugging).
use std::path::Path;

use clap::{Parser, Subcommand};

use crate::api::i2c::{self, SystemCommandRunner};

#[derive(Parser, Debug, PartialEq)]
#[command(name = "config-i2c", about = "I2C bus scanning tool")]
pub struct I2cArgs {
    #[command(subcommand)]
    pub command: I2cCommand,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum I2cCommand {
    /// Scan an I2C bus for devices
    Scan {
        /// I2C bus number to scan
        #[arg(long, default_value_t = 1)]
        bus: i32,
    },
    /// Show I2C bus info, including a device scan if available
    Info {
        /// I2C bus number to inspect
        #[arg(long, default_value_t = 1)]
        bus: i32,
    },
}

/// Run the i2c CLI. Returns the process exit code.
pub fn run(args: &I2cArgs) -> i32 {
    let runner = SystemCommandRunner;

    match &args.command {
        I2cCommand::Scan { bus } => {
            let bus_number = match i2c::validate_bus_number(*bus) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            match i2c::scan_i2c_bus(&runner, bus_number) {
                Ok(result) => {
                    println!("Bus {} (scan range {}):", result.bus_number, result.scan_range);
                    println!("  Detected devices: {}", if result.detected_devices.is_empty() { "none".to_string() } else { result.detected_devices.join(", ") });
                    println!("  Kernel-used addresses: {}", if result.kernel_used.is_empty() { "none".to_string() } else { result.kernel_used.join(", ") });
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        I2cCommand::Info { bus } => match i2c::get_i2c_info(&runner, Path::new("/"), *bus) {
            Ok(info) => {
                println!("Bus {}: {}", info.bus_number, info.bus_path);
                println!("  Exists: {}", info.bus_exists);
                println!("  i2cdetect available: {}", info.i2cdetect_available);
                if let Some(err) = &info.error {
                    println!("  Error: {err}");
                    1
                } else {
                    println!("  Detected devices: {}", if info.detected_devices.is_empty() { "none".to_string() } else { info.detected_devices.join(", ") });
                    println!("  Kernel-used addresses: {}", if info.kernel_used.is_empty() { "none".to_string() } else { info.kernel_used.join(", ") });
                    0
                }
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_command_with_default_bus() {
        let args = I2cArgs::parse_from(["config-i2c", "scan"]);
        assert_eq!(args.command, I2cCommand::Scan { bus: 1 });
    }

    #[test]
    fn parses_info_command_with_explicit_bus() {
        let args = I2cArgs::parse_from(["config-i2c", "info", "--bus", "3"]);
        assert_eq!(args.command, I2cCommand::Info { bus: 3 });
    }

    #[test]
    fn run_scan_rejects_invalid_bus_number() {
        let args = I2cArgs::parse_from(["config-i2c", "scan", "--bus", "42"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_info_returns_zero_or_one_without_panicking() {
        let args = I2cArgs::parse_from(["config-i2c", "info", "--bus", "1"]);
        // Result depends on real /dev/i2c-1 + i2cdetect availability in the
        // test environment; only exit-code stability (no panic) is asserted.
        let code = run(&args);
        assert!(code == 0 || code == 1);
    }
}
