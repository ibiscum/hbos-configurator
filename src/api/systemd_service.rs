//! Port of `configurator/systemd_service.py`: manages systemd services, including
//! HiFiBerry user-context services detected via `/etc/hifiberry.user`.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use crate::api::wifi::CommandRunner;

fn safe_service_name_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_.@:-]+$").unwrap())
}

/// Normalize and validate a service name before using it in commands/paths.
pub fn normalize_service_name(service_name: &str) -> Option<String> {
    let normalized = service_name.strip_suffix(".service").unwrap_or(service_name);
    if normalized.is_empty() || !safe_service_name_regex().is_match(normalized) {
        tracing::warn!("Rejected invalid service name: {}", service_name);
        return None;
    }
    Some(normalized.to_string())
}

fn run_command(runner: &dyn CommandRunner, command: &[&str]) -> (bool, String, String) {
    match runner.run(command) {
        Some(out) => (out.success(), out.stdout.trim().to_string(), out.stderr.trim().to_string()),
        None => (false, String::new(), format!("Error running command {}", command.join(" "))),
    }
}

/// Resolved HiFiBerry user context used to run "--user" systemd services.
#[derive(Debug, Clone, PartialEq)]
pub struct UserContext {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub runtime_dir: String,
}

/// Read `/etc/hifiberry.user` and resolve the configured user's uid/gid via `/etc/passwd`.
pub fn detect_user_service_user(root: &Path) -> Option<UserContext> {
    let content = fs::read_to_string(root.join("etc/hifiberry.user")).ok()?;
    let username = content
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;

    let passwd = fs::read_to_string(root.join("etc/passwd")).ok()?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4 && fields[0] == username {
            let uid: u32 = fields[2].parse().ok()?;
            let gid: u32 = fields[3].parse().ok()?;
            return Some(UserContext {
                name: username.to_string(),
                uid,
                gid,
                runtime_dir: format!("/run/user/{}", uid),
            });
        }
    }
    tracing::warn!("User in /etc/hifiberry.user does not exist: {}", username);
    None
}

/// Build the `systemd-run ... systemctl --user <args>` command for a HiFiBerry user service.
fn user_systemctl_cmd(user: &UserContext, args: &[&str]) -> Vec<String> {
    let mut cmd = vec![
        "systemd-run".to_string(),
        "--uid".to_string(),
        user.uid.to_string(),
        "--gid".to_string(),
        user.gid.to_string(),
        "--setenv".to_string(),
        format!("XDG_RUNTIME_DIR={}", user.runtime_dir),
        "--pipe".to_string(),
        "--wait".to_string(),
        "--quiet".to_string(),
        "--collect".to_string(),
        "systemctl".to_string(),
        "--user".to_string(),
    ];
    cmd.extend(args.iter().map(|s| s.to_string()));
    cmd
}

fn strip_service_suffix(name: &str) -> &str {
    name.strip_suffix(".service").unwrap_or(name)
}

/// Parse `systemctl list-units --type=service --no-legend` output into a name->env map.
fn parse_service_environment_lines(stdout: &str, environment: &str, map: &mut HashMap<String, String>) {
    for line in stdout.trim().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Skip ghost entries where LOAD is "not-found".
        if parts.len() >= 2 && parts[1] == "not-found" {
            continue;
        }
        if let Some(name) = parts.first() {
            map.insert(strip_service_suffix(name).to_string(), environment.to_string());
        }
    }
}

fn build_service_environment_map(runner: &dyn CommandRunner, user: Option<&UserContext>) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let system_cmd = ["systemctl", "list-units", "--type=service", "--no-pager", "--all", "--plain", "--no-legend"];
    if let Some(out) = runner.run(&system_cmd) {
        if out.success() {
            parse_service_environment_lines(&out.stdout, "system", &mut map);
        }
    }

    if let Some(user) = user {
        let cmd = user_systemctl_cmd(user, &["list-units", "--type=service", "--no-pager", "--all", "--plain", "--no-legend"]);
        let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        if let Some(out) = runner.run(&cmd_refs) {
            if out.success() {
                parse_service_environment_lines(&out.stdout, "user", &mut map);
            }
        }
    }

    map
}

/// A single entry from `systemctl list-units`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServiceEntry {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    pub environment: String,
}

fn parse_service_list(stdout: &str, environment: &str, pattern: Option<&str>) -> Vec<ServiceEntry> {
    let mut services = Vec::new();
    let mut start_parsing = false;

    for line in stdout.lines() {
        if line.starts_with("UNIT") {
            start_parsing = true;
            continue;
        }
        if !start_parsing {
            continue;
        }
        if line.trim().is_empty() || line.starts_with("LOAD =") || line.starts_with('\u{25cf}') {
            break;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let name = parts[0];
            if let Some(pattern) = pattern {
                if !pattern.is_empty() && !name.contains(pattern) {
                    continue;
                }
            }
            services.push(ServiceEntry {
                name: name.to_string(),
                load: parts[1].to_string(),
                active: parts[2].to_string(),
                sub: parts[3].to_string(),
                description: parts[4..].join(" "),
                environment: environment.to_string(),
            });
        }
    }

    services
}

/// Machine-readable status of a single service.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServiceStatus {
    pub service_name: String,
    pub active: String,
    pub enabled: String,
    pub status_output: String,
    pub status_available: bool,
    pub environment: String,
}

/// Manages systemd services in both the system bus and the HiFiBerry user context.
pub struct SystemdServiceManager<'a> {
    runner: &'a dyn CommandRunner,
    root: PathBuf,
    pub user: Option<UserContext>,
    service_environments: RefCell<HashMap<String, String>>,
}

impl<'a> SystemdServiceManager<'a> {
    /// Construct a manager rooted at `/`, auto-detecting the user context and
    /// building the service environment map (mirrors Python's `__init__`).
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self::with_root(runner, "/")
    }

    /// Like [`Self::new`], but reads `/etc/hifiberry.user` and `/etc/passwd`
    /// relative to `root` so tests can point at a fixture directory.
    pub fn with_root(runner: &'a dyn CommandRunner, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let user = detect_user_service_user(&root);
        let service_environments = build_service_environment_map(runner, user.as_ref());
        Self {
            runner,
            root,
            user,
            service_environments: RefCell::new(service_environments),
        }
    }

    /// Construct a manager without auto-detection, for tests that want full
    /// control over `user` and the service environment map.
    pub fn bare(runner: &'a dyn CommandRunner, root: impl Into<PathBuf>) -> Self {
        Self {
            runner,
            root: root.into(),
            user: None,
            service_environments: RefCell::new(HashMap::new()),
        }
    }

    fn get_service_environment(&self, service_name: &str) -> Option<String> {
        let normalized = normalize_service_name(service_name)?;

        if let Some(env) = self.service_environments.borrow().get(&normalized) {
            return Some(env.clone());
        }

        let service_file = format!("{}.service", normalized);
        let mut user_paths = vec![
            self.root.join("usr/lib/systemd/user").join(&service_file),
            self.root.join("etc/systemd/user").join(&service_file),
        ];
        if let Some(user) = &self.user {
            user_paths.push(PathBuf::from(&user.runtime_dir).join("systemd/user").join(&service_file));
        }
        for p in &user_paths {
            if p.exists() {
                self.service_environments.borrow_mut().insert(normalized.clone(), "user".to_string());
                return Some("user".to_string());
            }
        }

        let system_paths = [
            self.root.join("usr/lib/systemd/system").join(&service_file),
            self.root.join("etc/systemd/system").join(&service_file),
            self.root.join("lib/systemd/system").join(&service_file),
        ];
        for p in &system_paths {
            if p.exists() {
                self.service_environments.borrow_mut().insert(normalized.clone(), "system".to_string());
                return Some("system".to_string());
            }
        }

        None
    }

    fn run_service_cmd(&self, args: &[&str], service_name: Option<&str>) -> (bool, String, String) {
        if let Some(name) = service_name {
            if normalize_service_name(name).is_none() {
                return (false, String::new(), format!("Invalid service name: {}", name));
            }
            if self.get_service_environment(name).as_deref() == Some("user") {
                if let Some(user) = &self.user {
                    let full = user_systemctl_cmd(user, args);
                    let refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
                    return run_command(self.runner, &refs);
                }
            }
        }

        let mut cmd = vec!["systemctl"];
        cmd.extend_from_slice(args);
        run_command(self.runner, &cmd)
    }

    fn error_message(stdout: &str, stderr: &str) -> String {
        if !stderr.is_empty() {
            stderr.to_string()
        } else {
            stdout.to_string()
        }
    }

    pub fn enable(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["enable", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' enabled successfully", service_name))
        } else {
            (false, format!("Failed to enable service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn disable(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["disable", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' disabled successfully", service_name))
        } else {
            (false, format!("Failed to disable service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn start(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["start", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' started successfully", service_name))
        } else {
            (false, format!("Failed to start service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn stop(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["stop", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' stopped successfully", service_name))
        } else {
            (false, format!("Failed to stop service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn restart(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["restart", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' restarted successfully", service_name))
        } else {
            (false, format!("Failed to restart service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn reload(&self, service_name: &str) -> (bool, String) {
        let (success, stdout, stderr) = self.run_service_cmd(&["reload", service_name], Some(service_name));
        if success {
            (true, format!("Service '{}' reloaded successfully", service_name))
        } else {
            (false, format!("Failed to reload service '{}': {}", service_name, Self::error_message(&stdout, &stderr)))
        }
    }

    pub fn enable_now(&self, service_name: &str) -> (bool, String) {
        let (enabled, enable_msg) = self.enable(service_name);
        if !enabled {
            return (false, enable_msg);
        }
        let (started, start_msg) = self.start(service_name);
        if !started {
            return (false, format!("Service enabled but failed to start: {}", start_msg));
        }
        (true, format!("Service '{}' enabled and started successfully", service_name))
    }

    pub fn disable_now(&self, service_name: &str) -> (bool, String) {
        let (stopped, stop_msg) = self.stop(service_name);
        if !stopped {
            return (false, stop_msg);
        }
        let (disabled, disable_msg) = self.disable(service_name);
        if !disabled {
            return (false, format!("Service stopped but failed to disable: {}", disable_msg));
        }
        (true, format!("Service '{}' stopped and disabled successfully", service_name))
    }

    pub fn status(&self, service_name: &str) -> (bool, ServiceStatus) {
        let (success, stdout, stderr) = self.run_service_cmd(&["status", service_name], Some(service_name));
        let (is_active_success, is_active_stdout, _) = self.run_service_cmd(&["is-active", service_name], Some(service_name));
        let (is_enabled_success, is_enabled_stdout, _) = self.run_service_cmd(&["is-enabled", service_name], Some(service_name));

        let status = ServiceStatus {
            service_name: service_name.to_string(),
            active: if is_active_success { is_active_stdout } else { "unknown".to_string() },
            enabled: if is_enabled_success { is_enabled_stdout } else { "unknown".to_string() },
            status_output: if success { stdout } else { stderr },
            status_available: success,
            environment: self.get_service_environment(service_name).unwrap_or_else(|| "unknown".to_string()),
        };

        (success, status)
    }

    pub fn is_active(&self, service_name: &str) -> bool {
        let (success, stdout, _) = self.run_service_cmd(&["is-active", service_name], Some(service_name));
        success && stdout == "active"
    }

    pub fn is_enabled(&self, service_name: &str) -> bool {
        let (success, stdout, _) = self.run_service_cmd(&["is-enabled", service_name], Some(service_name));
        success && stdout == "enabled"
    }

    pub fn list_services(&self, pattern: Option<&str>) -> (bool, Vec<ServiceEntry>) {
        let mut all_services = Vec::new();
        let mut any_success = false;

        let mut system_cmd = vec!["systemctl", "list-units", "--type=service", "--no-pager"];
        if pattern.is_some() {
            system_cmd.push("--all");
        }
        let (success, stdout, _) = run_command(self.runner, &system_cmd);
        if success {
            any_success = true;
            all_services.extend(parse_service_list(&stdout, "system", pattern));
        }

        if let Some(user) = &self.user {
            let mut args = vec!["list-units", "--type=service", "--no-pager"];
            if pattern.is_some() {
                args.push("--all");
            }
            let cmd = user_systemctl_cmd(user, &args);
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            let (success, stdout, _) = run_command(self.runner, &cmd_refs);
            if success {
                any_success = true;
                all_services.extend(parse_service_list(&stdout, "user", pattern));
            }
        }

        (any_success, all_services)
    }

    pub fn daemon_reload(&self) -> (bool, String) {
        let (success, stdout, stderr) = run_command(self.runner, &["systemctl", "daemon-reload"]);
        if success {
            (true, "Systemd daemon configuration reloaded successfully".to_string())
        } else {
            (false, format!("Failed to reload systemd daemon configuration: {}", Self::error_message(&stdout, &stderr)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::wifi::{CommandOutput, FakeCommandRunner};
    use std::fs;

    fn cp(status: i32, stdout: &str, stderr: &str) -> Option<CommandOutput> {
        Some(CommandOutput::new(status, stdout, stderr))
    }

    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn normalize_service_name_strips_suffix() {
        assert_eq!(normalize_service_name("foo.service").as_deref(), Some("foo"));
        assert_eq!(normalize_service_name("foo").as_deref(), Some("foo"));
    }

    #[test]
    fn normalize_service_name_rejects_invalid_characters() {
        assert_eq!(normalize_service_name("foo; rm -rf /"), None);
        assert_eq!(normalize_service_name(""), None);
    }

    #[test]
    fn detect_user_service_user_sets_gid() {
        let dir = fixture();
        write(dir.path(), "etc/hifiberry.user", "# comment\nalice\n");
        write(dir.path(), "etc/passwd", "root:x:0:0:root:/root:/bin/bash\nalice:x:1234:4321:Alice:/home/alice:/bin/bash\n");

        let user = detect_user_service_user(dir.path()).expect("user detected");
        assert_eq!(user.name, "alice");
        assert_eq!(user.uid, 1234);
        assert_eq!(user.gid, 4321);
        assert_eq!(user.runtime_dir, "/run/user/1234");
    }

    #[test]
    fn detect_user_service_user_missing_file_returns_none() {
        let dir = fixture();
        assert_eq!(detect_user_service_user(dir.path()), None);
    }

    #[test]
    fn detect_user_service_user_unknown_user_returns_none() {
        let dir = fixture();
        write(dir.path(), "etc/hifiberry.user", "bob\n");
        write(dir.path(), "etc/passwd", "root:x:0:0:root:/root:/bin/bash\n");
        assert_eq!(detect_user_service_user(dir.path()), None);
    }

    #[test]
    fn status_propagates_failed_status() {
        let runner = FakeCommandRunner::new(vec![cp(1, "", "Unit missing"), cp(1, "", ""), cp(1, "", "")]);
        let manager = SystemdServiceManager::bare(&runner, "/");

        let (success, status) = manager.status("missing-service");

        assert!(!success);
        assert!(!status.status_available);
        assert_eq!(status.status_output, "Unit missing");
    }

    #[test]
    fn run_service_cmd_user_uses_user_gid() {
        let user = UserContext {
            name: "alice".to_string(),
            uid: 1001,
            gid: 1005,
            runtime_dir: "/run/user/1001".to_string(),
        };
        let runner = FakeCommandRunner::new(vec![cp(0, "", "")]);
        let mut environments = HashMap::new();
        environments.insert("demo".to_string(), "user".to_string());
        let manager = SystemdServiceManager {
            runner: &runner,
            root: PathBuf::from("/"),
            user: Some(user),
            service_environments: RefCell::new(environments),
        };

        let (success, _, _) = manager.run_service_cmd(&["status", "demo.service"], Some("demo.service"));
        assert!(success);
    }

    #[test]
    fn list_services_returns_false_when_all_commands_fail() {
        let user = UserContext {
            name: "alice".to_string(),
            uid: 1001,
            gid: 1005,
            runtime_dir: "/run/user/1001".to_string(),
        };
        let runner = FakeCommandRunner::new(vec![cp(1, "", "error"), cp(1, "", "error")]);
        let manager = SystemdServiceManager {
            runner: &runner,
            root: PathBuf::from("/"),
            user: Some(user),
            service_environments: RefCell::new(HashMap::new()),
        };

        let (success, services) = manager.list_services(None);
        assert!(!success);
        assert!(services.is_empty());
    }

    #[test]
    fn list_services_user_listing_uses_systemd_run() {
        let user = UserContext {
            name: "alice".to_string(),
            uid: 1001,
            gid: 1005,
            runtime_dir: "/run/user/1001".to_string(),
        };
        let system_stdout = "UNIT LOAD ACTIVE SUB DESCRIPTION\nalpha.service loaded active running Alpha\n";
        let user_stdout = "UNIT LOAD ACTIVE SUB DESCRIPTION\nbeta.service loaded inactive dead Beta\n";
        let runner = FakeCommandRunner::new(vec![cp(0, system_stdout, ""), cp(0, user_stdout, "")]);
        let manager = SystemdServiceManager {
            runner: &runner,
            root: PathBuf::from("/"),
            user: Some(user),
            service_environments: RefCell::new(HashMap::new()),
        };

        let (success, services) = manager.list_services(Some(""));
        assert!(success);
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].environment, "system");
        assert_eq!(services[1].environment, "user");
    }

    #[test]
    fn enable_now_reports_start_failure_after_successful_enable() {
        let runner = FakeCommandRunner::new(vec![cp(0, "", ""), cp(1, "", "start failed")]);
        let manager = SystemdServiceManager::bare(&runner, "/");
        let (success, message) = manager.enable_now("demo");
        assert!(!success);
        assert!(message.contains("Service enabled but failed to start"));
    }

    #[test]
    fn is_active_and_is_enabled_check_exact_output() {
        let runner = FakeCommandRunner::new(vec![cp(0, "active", ""), cp(0, "enabled", "")]);
        let manager = SystemdServiceManager::bare(&runner, "/");
        assert!(manager.is_active("demo"));
        assert!(manager.is_enabled("demo"));
    }

    #[test]
    fn daemon_reload_reports_failure_message() {
        let runner = FakeCommandRunner::new(vec![cp(1, "", "denied")]);
        let manager = SystemdServiceManager::bare(&runner, "/");
        let (success, message) = manager.daemon_reload();
        assert!(!success);
        assert!(message.contains("denied"));
    }

    #[test]
    fn get_service_environment_late_detects_from_disk() {
        let dir = fixture();
        write(dir.path(), "etc/systemd/system/demo.service", "[Unit]\n");
        let runner = FakeCommandRunner::new(vec![]);
        let manager = SystemdServiceManager::bare(&runner, dir.path());
        assert_eq!(manager.get_service_environment("demo"), Some("system".to_string()));
    }
}
