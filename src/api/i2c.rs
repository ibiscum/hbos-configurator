//! Port of `configurator/i2c.py` (I2C bus scanning/info).
//!
//! The Python original uses the `smbus2` library for direct raw I2C ioctl
//! access. No equivalent crate is part of this project's dependencies, so
//! (consistent with how `soundcard_detector`'s I2C probing was ported)
//! scanning goes through the `i2cdetect` command-line tool instead: its
//! `UU` marker for kernel-claimed addresses maps directly to Python's
//! `kernel_used` list (sourced there from `/sys/bus/i2c/devices/...`), and
//! its plain hex cells map to `detected_devices`.
use std::path::Path;
use std::process::Command;

use serde::Serialize;

/// Validate a bus number before using it in device or sysfs paths.
pub fn validate_bus_number(bus_number: i32) -> Result<u32, String> {
    if !(0..=10).contains(&bus_number) {
        return Err("I2C bus number must be between 0 and 10".to_string());
    }
    Ok(bus_number as u32)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

/// Runs external commands and checks their availability (mirrors
/// `shutil.which`/`subprocess.run`).
pub trait CommandRunner: Send + Sync {
    fn which(&self, cmd: &str) -> bool;
    fn run(&self, args: &[&str]) -> Option<CommandOutput>;
}

/// Real implementation that spawns processes via [`std::process::Command`].
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn which(&self, cmd: &str) -> bool {
        Command::new("which").arg(cmd).output().map(|out| out.status.success()).unwrap_or(false)
    }

    fn run(&self, args: &[&str]) -> Option<CommandOutput> {
        let (program, rest) = args.split_first()?;
        Command::new(program).args(rest).output().ok().map(|out| CommandOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}

/// Parse `i2cdetect -y <bus>` grid output into (detected_devices, kernel_used),
/// both formatted as `"0xNN"`, sorted.
fn parse_i2cdetect_output(output: &str) -> (Vec<String>, Vec<String>) {
    let mut detected = Vec::new();
    let mut kernel_used = Vec::new();

    for line in output.lines() {
        let Some((row_label, rest)) = line.split_once(':') else { continue };
        let Ok(row_base) = u32::from_str_radix(row_label.trim(), 16) else { continue };

        for (col, token) in rest.split_whitespace().enumerate() {
            let addr = row_base + col as u32;
            if token == "--" {
                continue;
            }
            if token.eq_ignore_ascii_case("UU") {
                kernel_used.push(format!("0x{addr:02x}"));
            } else if u32::from_str_radix(token, 16).is_ok() {
                detected.push(format!("0x{addr:02x}"));
            }
        }
    }

    detected.sort();
    kernel_used.sort();
    (detected, kernel_used)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct I2cScanResult {
    pub bus_number: u32,
    pub detected_devices: Vec<String>,
    pub kernel_used: Vec<String>,
    pub scan_range: String,
}

/// Scan an I2C bus for devices and detect which addresses are kernel-claimed.
pub fn scan_i2c_bus(runner: &dyn CommandRunner, bus_number: u32) -> Result<I2cScanResult, String> {
    if !runner.which("i2cdetect") {
        return Err("i2cdetect command not found. Install with: sudo apt install i2c-tools".to_string());
    }

    let bus_str = bus_number.to_string();
    let output = runner.run(&["i2cdetect", "-y", &bus_str]).ok_or_else(|| "i2cdetect command failed to execute".to_string())?;
    if !output.success() {
        return Err(format!("i2cdetect exited with an error: {}", output.stderr.trim()));
    }

    let (detected_devices, kernel_used) = parse_i2cdetect_output(&output.stdout);
    Ok(I2cScanResult { bus_number, detected_devices, kernel_used, scan_range: "0x03-0x77".to_string() })
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct I2cInfo {
    pub bus_number: u32,
    pub bus_path: String,
    pub bus_exists: bool,
    pub i2cdetect_available: bool,
    pub detected_devices: Vec<String>,
    pub kernel_used: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Get I2C bus information, including a device scan if the bus and tooling are available.
pub fn get_i2c_info(runner: &dyn CommandRunner, root: &Path, bus_number: i32) -> Result<I2cInfo, String> {
    let bus_number = validate_bus_number(bus_number)?;
    let bus_path = format!("/dev/i2c-{bus_number}");
    let bus_exists = root.join(bus_path.trim_start_matches('/')).exists();
    let i2cdetect_available = runner.which("i2cdetect");

    let mut info = I2cInfo { bus_number, bus_path: bus_path.clone(), bus_exists, i2cdetect_available, ..Default::default() };

    if !bus_exists {
        info.error = Some(format!("I2C bus {bus_number} not found. Make sure I2C is enabled."));
        return Ok(info);
    }

    if !i2cdetect_available {
        info.error = Some("i2cdetect command not found. Cannot scan I2C bus.".to_string());
        return Ok(info);
    }

    match scan_i2c_bus(runner, bus_number) {
        Ok(scan) => {
            info.detected_devices = scan.detected_devices;
            info.kernel_used = scan.kernel_used;
            info.scan_range = Some(scan.scan_range);
        }
        Err(e) => info.error = Some(e),
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    struct StubRunner {
        which: bool,
        response: Option<CommandOutput>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl CommandRunner for StubRunner {
        fn which(&self, _cmd: &str) -> bool {
            self.which
        }
        fn run(&self, args: &[&str]) -> Option<CommandOutput> {
            self.calls.lock().unwrap().push(args.iter().map(|s| s.to_string()).collect());
            self.response.clone()
        }
    }

    #[test]
    fn validate_bus_number_accepts_in_range() {
        assert_eq!(validate_bus_number(1), Ok(1));
        assert_eq!(validate_bus_number(0), Ok(0));
        assert_eq!(validate_bus_number(10), Ok(10));
    }

    #[test]
    fn validate_bus_number_rejects_out_of_range() {
        assert!(validate_bus_number(-1).is_err());
        assert!(validate_bus_number(11).is_err());
    }

    #[test]
    fn parse_i2cdetect_output_extracts_detected_and_kernel_used() {
        let output = [
            "     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f",
            "00:          -- -- -- -- -- -- -- -- -- -- -- -- --",
            "10: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- --",
            "40: -- -- -- -- -- -- -- -- UU -- -- -- -- -- -- --",
            "50: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 5f",
            "70: -- -- -- -- -- -- -- --",
        ]
        .join("\n");

        let (detected, kernel_used) = parse_i2cdetect_output(&output);
        assert_eq!(detected, vec!["0x5f".to_string()]);
        assert_eq!(kernel_used, vec!["0x48".to_string()]);
    }

    #[test]
    fn parse_i2cdetect_output_ignores_header_and_dashes_only() {
        let output = "     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f\n00:          -- -- -- -- -- -- -- -- -- -- -- -- --\n";
        let (detected, kernel_used) = parse_i2cdetect_output(output);
        assert!(detected.is_empty());
        assert!(kernel_used.is_empty());
    }

    #[test]
    fn scan_i2c_bus_returns_error_when_i2cdetect_missing() {
        let runner = StubRunner { which: false, response: None, calls: Mutex::new(Vec::new()) };
        let err = scan_i2c_bus(&runner, 1).unwrap_err();
        assert!(err.contains("i2cdetect command not found"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn scan_i2c_bus_returns_error_on_nonzero_exit() {
        let runner = StubRunner { which: true, response: Some(CommandOutput { status: 1, stdout: String::new(), stderr: "no such bus".to_string() }), calls: Mutex::new(Vec::new()) };
        let err = scan_i2c_bus(&runner, 1).unwrap_err();
        assert!(err.contains("no such bus"));
    }

    #[test]
    fn scan_i2c_bus_success_parses_output() {
        let output = "40: -- -- -- -- -- -- -- -- UU -- -- -- -- -- -- --\n".to_string();
        let runner = StubRunner { which: true, response: Some(CommandOutput { status: 0, stdout: output, stderr: String::new() }), calls: Mutex::new(Vec::new()) };
        let result = scan_i2c_bus(&runner, 1).unwrap();
        assert_eq!(result.bus_number, 1);
        assert_eq!(result.kernel_used, vec!["0x48".to_string()]);
        assert_eq!(result.scan_range, "0x03-0x77");
    }

    #[test]
    fn get_i2c_info_bus_missing_returns_error() {
        let dir = fixture();
        let runner = StubRunner { which: true, response: None, calls: Mutex::new(Vec::new()) };
        let info = get_i2c_info(&runner, dir.path(), 1).unwrap();
        assert!(!info.bus_exists);
        assert!(info.error.is_some());
    }

    #[test]
    fn get_i2c_info_i2cdetect_unavailable_returns_error() {
        let dir = fixture();
        std::fs::create_dir_all(dir.path().join("dev")).unwrap();
        std::fs::write(dir.path().join("dev/i2c-1"), "").unwrap();
        let runner = StubRunner { which: false, response: None, calls: Mutex::new(Vec::new()) };

        let info = get_i2c_info(&runner, dir.path(), 1).unwrap();
        assert!(info.bus_exists);
        assert!(!info.i2cdetect_available);
        assert!(info.error.as_ref().unwrap().contains("i2cdetect command not found"));
    }

    #[test]
    fn get_i2c_info_merges_scan_result() {
        let dir = fixture();
        std::fs::create_dir_all(dir.path().join("dev")).unwrap();
        std::fs::write(dir.path().join("dev/i2c-1"), "").unwrap();
        let output = "40: -- -- -- -- -- -- -- -- 48 -- -- -- -- -- -- --\n".to_string();
        let runner = StubRunner { which: true, response: Some(CommandOutput { status: 0, stdout: output, stderr: String::new() }), calls: Mutex::new(Vec::new()) };

        let info = get_i2c_info(&runner, dir.path(), 1).unwrap();
        assert_eq!(info.bus_number, 1);
        assert_eq!(info.detected_devices, vec!["0x48".to_string()]);
        assert!(info.error.is_none());
    }

    #[test]
    fn get_i2c_info_rejects_invalid_bus_number() {
        let dir = fixture();
        let runner = StubRunner { which: true, response: None, calls: Mutex::new(Vec::new()) };
        assert!(get_i2c_info(&runner, dir.path(), 42).is_err());
    }
}
