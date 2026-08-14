//! Port of `configurator/pipewire.py` (PipeWire volume control via `pw-cli`).
use std::process::Command;

use regex::Regex;

#[allow(dead_code)]
const PW_CLI_TIMEOUT_SECS: u64 = 5;

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

/// Runs external commands (mirrors `subprocess.run`).
pub trait CommandRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> Option<CommandOutput>;
}

/// Real implementation that spawns `pw-cli` via [`std::process::Command`].
///
/// The Python original enforces a 5s timeout on `pw-cli`; that isn't
/// portable without an extra dependency here, so the command simply runs to
/// completion (see [`PW_CLI_TIMEOUT_SECS`] for the value it mirrors).
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, args: &[&str]) -> Option<CommandOutput> {
        let (program, rest) = args.split_first()?;
        Command::new(program).args(rest).output().ok().map(|out| CommandOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}

fn run_pw_cli(runner: &dyn CommandRunner, args: &[&str]) -> Option<String> {
    let mut cmd = vec!["pw-cli"];
    cmd.extend_from_slice(args);
    match runner.run(&cmd) {
        Some(output) if output.success() => Some(output.stdout),
        _ => None,
    }
}

/// Returns a list of all PipeWire volume control names.
pub fn get_volume_controls(runner: &dyn CommandRunner) -> Vec<String> {
    let Some(output) = run_pw_cli(runner, &["list", "Node"]) else {
        return Vec::new();
    };
    let name_re = Regex::new(r#"^\s*name\s*=\s*"([^"]+)"\s*$"#).unwrap();
    output.lines().filter_map(|line| name_re.captures(line).map(|c| c[1].to_string())).collect()
}

/// Gets the volume for the given PipeWire control name, as a float between
/// 0.0 and 1.0, or `None` if not found.
pub fn get_volume(runner: &dyn CommandRunner, control_name: &str) -> Option<f64> {
    let output = run_pw_cli(runner, &["info", control_name])?;
    let volume_re = Regex::new(r"^\s*volume\s*=\s*([0-9]*\.?[0-9]+)\s*$").unwrap();
    output.lines().find_map(|line| volume_re.captures(line).and_then(|c| c[1].parse::<f64>().ok()))
}

/// Sets the volume (0.0-1.0) for the given PipeWire control name.
pub fn set_volume(runner: &dyn CommandRunner, control_name: &str, volume: f64) -> bool {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return false;
    }
    runner.run(&["pw-cli", "set", control_name, "volume", &volume.to_string()]).is_some_and(|out| out.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct StubRunner {
        response: Option<CommandOutput>,
        calls: Mutex<u32>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, _args: &[&str]) -> Option<CommandOutput> {
            *self.calls.lock().unwrap() += 1;
            self.response.clone()
        }
    }

    #[test]
    fn run_pw_cli_success() {
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: "ok\n".to_string(), stderr: String::new() }), calls: Mutex::new(0) };
        assert_eq!(run_pw_cli(&runner, &["list", "Node"]), Some("ok\n".to_string()));
        assert_eq!(*runner.calls.lock().unwrap(), 1);
    }

    #[test]
    fn run_pw_cli_command_missing_returns_none() {
        let runner = StubRunner { response: None, calls: Mutex::new(0) };
        assert_eq!(run_pw_cli(&runner, &["list", "Node"]), None);
    }

    #[test]
    fn run_pw_cli_nonzero_exit_returns_none() {
        let runner = StubRunner { response: Some(CommandOutput { status: 1, stdout: String::new(), stderr: "boom".to_string() }), calls: Mutex::new(0) };
        assert_eq!(run_pw_cli(&runner, &["list", "Node"]), None);
    }

    #[test]
    fn get_volume_controls_parses_name_lines_only() {
        let output = ["    name = \"alsa_output.main\"", "    nick = \"Main\"", "    object.serial = \"22\"", "    node.name = \"not-this\""].join("\n");
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: output, stderr: String::new() }), calls: Mutex::new(0) };
        assert_eq!(get_volume_controls(&runner), vec!["alsa_output.main".to_string()]);
    }

    #[test]
    fn get_volume_controls_no_output() {
        let runner = StubRunner { response: None, calls: Mutex::new(0) };
        assert_eq!(get_volume_controls(&runner), Vec::<String>::new());
    }

    #[test]
    fn get_volume_success() {
        let output = ["\tid = 42", "\tvolume = 0.75"].join("\n");
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: output, stderr: String::new() }), calls: Mutex::new(0) };
        assert_eq!(get_volume(&runner, "alsa_output.main"), Some(0.75));
    }

    #[test]
    fn get_volume_ignores_non_volume_lines() {
        let output = ["\tvolume.base = 1.0", "\tvolumeStep = 0.01"].join("\n");
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: output, stderr: String::new() }), calls: Mutex::new(0) };
        assert_eq!(get_volume(&runner, "alsa_output.main"), None);
    }

    #[test]
    fn get_volume_no_output() {
        let runner = StubRunner { response: None, calls: Mutex::new(0) };
        assert_eq!(get_volume(&runner, "alsa_output.main"), None);
    }

    #[test]
    fn set_volume_success() {
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }), calls: Mutex::new(0) };
        assert!(set_volume(&runner, "alsa_output.main", 0.42));
        assert_eq!(*runner.calls.lock().unwrap(), 1);
    }

    #[test]
    fn set_volume_rejects_out_of_range() {
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }), calls: Mutex::new(0) };
        assert!(!set_volume(&runner, "alsa_output.main", 1.5));
        assert!(!set_volume(&runner, "alsa_output.main", -0.1));
        assert_eq!(*runner.calls.lock().unwrap(), 0);
    }

    #[test]
    fn set_volume_rejects_non_finite() {
        let runner = StubRunner { response: Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }), calls: Mutex::new(0) };
        assert!(!set_volume(&runner, "alsa_output.main", f64::NAN));
        assert!(!set_volume(&runner, "alsa_output.main", f64::INFINITY));
        assert_eq!(*runner.calls.lock().unwrap(), 0);
    }

    #[test]
    fn set_volume_subprocess_error_returns_false() {
        let runner = StubRunner { response: Some(CommandOutput { status: 1, stdout: String::new(), stderr: "denied".to_string() }), calls: Mutex::new(0) };
        assert!(!set_volume(&runner, "alsa_output.main", 0.5));
    }
}
