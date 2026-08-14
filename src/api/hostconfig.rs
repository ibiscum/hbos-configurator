//! Port of `configurator/hostconfig.py` (hostname validation/sanitization
//! and `/etc/hosts` management).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

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

/// Real implementation that spawns `hostnamectl` via [`std::process::Command`].
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

fn hosts_path(root: &Path) -> PathBuf {
    root.join("etc/hosts")
}

fn hosts_backup_path(root: &Path) -> PathBuf {
    root.join("etc/hosts.backup")
}

/// Read the contents of `/etc/hosts`, one entry per line (newline included).
pub fn read_hosts_file(root: &Path) -> Vec<String> {
    match fs::read_to_string(hosts_path(root)) {
        Ok(content) if !content.is_empty() => content.split_inclusive('\n').map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Write `lines` to `/etc/hosts`, first backing up the existing file.
///
/// Mirrors the Python original: if the existing file can't be read for the
/// backup, the whole write is aborted (returns `false`) without touching
/// the real hosts file.
pub fn write_hosts_file(root: &Path, lines: &[String]) -> bool {
    let Ok(content) = fs::read_to_string(hosts_path(root)) else {
        return false;
    };
    if fs::write(hosts_backup_path(root), content).is_err() {
        return false;
    }
    fs::write(hosts_path(root), lines.concat()).is_ok()
}

/// Update `/etc/hosts` when the hostname changes: removes old hostname
/// entries and adds the new hostname to the `127.0.0.1` entry.
///
/// This is designed to be resilient - individual failures (like removing
/// the old hostname) won't cause the entire operation to fail.
pub fn update_hosts_file(root: &Path, old_hostname: Option<&str>, new_hostname: &str) -> bool {
    let mut lines = read_hosts_file(root);
    if lines.is_empty() {
        lines = vec![
            "127.0.0.1\tlocalhost\n".to_string(),
            "::1\t\tlocalhost ip6-localhost ip6-loopback\n".to_string(),
            "ff02::1\t\tip6-allnodes\n".to_string(),
            "ff02::2\t\tip6-allrouters\n".to_string(),
        ];
    }

    let mut updated_lines: Vec<String> = Vec::new();
    let mut hostname_added = false;

    for line in &lines {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            updated_lines.push(line.clone());
            continue;
        }

        let parts: Vec<&str> = stripped.split_whitespace().collect();
        if parts.len() < 2 {
            updated_lines.push(line.clone());
            continue;
        }

        let ip = parts[0];
        let mut hostnames: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        if let Some(old) = old_hostname {
            hostnames.retain(|h| h != old);
        }

        if ip == "127.0.0.1" && !hostnames.iter().any(|h| h == new_hostname) && hostnames.iter().any(|h| h == "localhost") {
            hostnames.push(new_hostname.to_string());
            hostname_added = true;
        }

        if !hostnames.is_empty() {
            updated_lines.push(format!("{}\t{}\n", ip, hostnames.join(" ")));
        }
    }

    if !hostname_added {
        let localhost_line = updated_lines.iter().position(|l| l.contains("127.0.0.1") && l.contains("localhost"));
        if let Some(idx) = localhost_line {
            let parts: Vec<&str> = updated_lines[idx].trim().split_whitespace().collect();
            if parts.len() >= 2 {
                let mut hostnames: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                if !hostnames.iter().any(|h| h == new_hostname) {
                    hostnames.push(new_hostname.to_string());
                    updated_lines[idx] = format!("127.0.0.1\t{}\n", hostnames.join(" "));
                }
            }
        } else {
            updated_lines.insert(0, format!("127.0.0.1\tlocalhost {new_hostname}\n"));
        }
    }

    write_hosts_file(root, &updated_lines)
}

/// Get the current system hostname using `hostnamectl hostname`.
pub fn get_current_hostname(runner: &dyn CommandRunner) -> Option<String> {
    runner.run(&["hostnamectl", "hostname"]).filter(|o| o.success()).map(|o| o.stdout.trim().to_string())
}

/// Set the system hostname and update `/etc/hosts` accordingly.
///
/// Returns `true` if the hostname was set successfully; hosts-file update
/// failures are logged but don't affect the return value, since setting
/// the hostname is the critical operation.
pub fn set_hostname_with_hosts_update(runner: &dyn CommandRunner, root: &Path, new_hostname: &str) -> bool {
    let old_hostname = get_current_hostname(runner);

    let Some(output) = runner.run(&["hostnamectl", "set-hostname", new_hostname]) else {
        return false;
    };
    if !output.success() {
        return false;
    }

    update_hosts_file(root, old_hostname.as_deref(), new_hostname);
    true
}

/// Validate system hostname format according to RFC 1123.
pub fn validate_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.len() > 64 {
        return false;
    }
    if !Regex::new(r"^[a-zA-Z0-9.-]+$").unwrap().is_match(hostname) {
        return false;
    }
    if hostname.starts_with('-') || hostname.starts_with('.') || hostname.ends_with('-') || hostname.ends_with('.') {
        return false;
    }
    hostname.split('.').all(|label| !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-'))
}

/// Convert a pretty hostname into a valid system hostname (RFC 1123).
///
/// Falls back to `"hifiberry"` (truncated to `max_length`) if sanitization
/// results in an empty string or a leading hyphen.
pub fn sanitize_hostname(pretty_hostname: &str, max_length: i32) -> String {
    let max_length = if max_length < 1 { 1usize } else { max_length as usize };

    let mut hostname = pretty_hostname.to_lowercase().replace(' ', "-");
    hostname = Regex::new(r"[^a-z0-9-]").unwrap().replace_all(&hostname, "").to_string();
    hostname = Regex::new(r"-+").unwrap().replace_all(&hostname, "-").trim_matches('-').to_string();
    hostname = hostname.chars().take(max_length).collect();
    hostname = hostname.trim_end_matches('-').to_string();

    if hostname.is_empty() || hostname.starts_with('-') {
        hostname = "hifiberry".chars().take(max_length).collect();
    }
    hostname
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_hosts(dir: &Path, content: &str) {
        fs::create_dir_all(dir.join("etc")).unwrap();
        fs::write(hosts_path(dir), content).unwrap();
    }

    struct StubRunner {
        responses: Mutex<Vec<Option<CommandOutput>>>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, _args: &[&str]) -> Option<CommandOutput> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                None
            } else {
                responses.remove(0)
            }
        }
    }

    #[test]
    fn valid_single_and_multi_label_hostnames() {
        assert!(validate_hostname("localhost"));
        assert!(validate_hostname("my-host"));
        assert!(validate_hostname("host.example.com"));
        assert!(validate_hostname("HostName"));
    }

    #[test]
    fn invalid_hostnames_rejected() {
        assert!(!validate_hostname(""));
        assert!(!validate_hostname(&"a".repeat(65)));
        assert!(!validate_hostname("-hostname"));
        assert!(!validate_hostname("hostname-"));
        assert!(!validate_hostname("host@name"));
        assert!(!validate_hostname("host_name"));
        assert!(!validate_hostname("host name"));
    }

    #[test]
    fn label_length_boundaries() {
        assert!(validate_hostname(&"a".repeat(63)));
        assert!(!validate_hostname(&"a".repeat(64)));
    }

    #[test]
    fn sanitize_simple_and_spaces() {
        assert_eq!(sanitize_hostname("my-host", 64), "my-host");
        assert_eq!(sanitize_hostname("my host", 64), "my-host");
        assert_eq!(sanitize_hostname("MyHost", 64), "myhost");
    }

    #[test]
    fn sanitize_removes_special_characters() {
        assert_eq!(sanitize_hostname("my@host#name!", 64), "myhostname");
    }

    #[test]
    fn sanitize_collapses_multiple_hyphens() {
        assert!(sanitize_hostname("my   host", 64).contains("my-host"));
    }

    #[test]
    fn sanitize_trims_leading_and_trailing_hyphens() {
        assert_eq!(sanitize_hostname("-hostname", 64), "hostname");
        assert_eq!(sanitize_hostname("hostname-", 64), "hostname");
    }

    #[test]
    fn sanitize_truncates_long_hostname() {
        let result = sanitize_hostname(&"a".repeat(100), 64);
        assert!(result.len() <= 64);
    }

    #[test]
    fn sanitize_respects_custom_max_length() {
        let result = sanitize_hostname(&"a".repeat(50), 30);
        assert!(result.len() <= 30);
    }

    #[test]
    fn sanitize_clamps_non_positive_max_length() {
        assert_eq!(sanitize_hostname("my host", 0), "m");
        assert_eq!(sanitize_hostname("my host", -5), "m");
    }

    #[test]
    fn sanitize_falls_back_to_hifiberry() {
        assert_eq!(sanitize_hostname("!!!", 64), "hifiberry");
        assert_eq!(sanitize_hostname("---", 64), "hifiberry");
        assert_eq!(sanitize_hostname("!!!", 3), "hif");
    }

    #[test]
    fn sanitize_removes_unicode() {
        let result = sanitize_hostname("host-中文-name", 64);
        assert!(!result.contains('中'));
    }

    #[test]
    fn sanitize_then_validate_round_trip() {
        for input in ["My-Host!@#", "UPPERCASE", "with-spaces", "!!!", "host_name", "short-name"] {
            let sanitized = sanitize_hostname(input, 64);
            assert!(validate_hostname(&sanitized), "sanitized '{input}' to '{sanitized}' which is invalid");
        }
    }

    #[test]
    fn read_hosts_file_missing_returns_empty() {
        let dir = fixture();
        assert_eq!(read_hosts_file(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn read_hosts_file_success() {
        let dir = fixture();
        write_hosts(dir.path(), "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
        assert_eq!(read_hosts_file(dir.path()).len(), 2);
    }

    #[test]
    fn write_hosts_file_fails_when_original_missing() {
        let dir = fixture();
        assert!(!write_hosts_file(dir.path(), &["127.0.0.1\tlocalhost\n".to_string()]));
    }

    #[test]
    fn write_hosts_file_success_creates_backup_and_writes() {
        let dir = fixture();
        write_hosts(dir.path(), "old content\n");
        assert!(write_hosts_file(dir.path(), &["127.0.0.1\tlocalhost\n".to_string()]));
        assert_eq!(fs::read_to_string(hosts_path(dir.path())).unwrap(), "127.0.0.1\tlocalhost\n");
        assert_eq!(fs::read_to_string(hosts_backup_path(dir.path())).unwrap(), "old content\n");
    }

    #[test]
    fn update_hosts_file_adds_new_hostname() {
        let dir = fixture();
        write_hosts(dir.path(), "127.0.0.1\tlocalhost\n::1\t\tlocalhost ip6-localhost\n");
        assert!(update_hosts_file(dir.path(), None, "newhost"));
        let content = fs::read_to_string(hosts_path(dir.path())).unwrap();
        assert!(content.contains("127.0.0.1\tlocalhost newhost"));
    }

    #[test]
    fn update_hosts_file_removes_old_hostname() {
        let dir = fixture();
        write_hosts(dir.path(), "127.0.0.1\tlocalhost oldhost\n::1\t\tlocalhost\n");
        assert!(update_hosts_file(dir.path(), Some("oldhost"), "newhost"));
        let content = fs::read_to_string(hosts_path(dir.path())).unwrap();
        assert!(!content.contains("oldhost"));
        assert!(content.contains("newhost"));
    }

    #[test]
    fn update_hosts_file_empty_file_creates_default_structure() {
        let dir = fixture();
        write_hosts(dir.path(), "");
        assert!(update_hosts_file(dir.path(), None, "newhost"));
        let content = fs::read_to_string(hosts_path(dir.path())).unwrap();
        assert!(content.contains("newhost"));
    }

    #[test]
    fn update_hosts_file_preserves_comments() {
        let dir = fixture();
        write_hosts(dir.path(), "# This is a comment\n127.0.0.1\tlocalhost\n");
        assert!(update_hosts_file(dir.path(), None, "newhost"));
        let content = fs::read_to_string(hosts_path(dir.path())).unwrap();
        assert!(content.contains("# This is a comment"));
    }

    #[test]
    fn get_current_hostname_success() {
        let runner = StubRunner { responses: Mutex::new(vec![Some(CommandOutput { status: 0, stdout: "myhostname\n".to_string(), stderr: String::new() })]) };
        assert_eq!(get_current_hostname(&runner), Some("myhostname".to_string()));
    }

    #[test]
    fn get_current_hostname_command_failure_returns_none() {
        let runner = StubRunner { responses: Mutex::new(vec![Some(CommandOutput { status: 1, stdout: String::new(), stderr: "failed".to_string() })]) };
        assert_eq!(get_current_hostname(&runner), None);
    }

    #[test]
    fn get_current_hostname_missing_binary_returns_none() {
        let runner = StubRunner { responses: Mutex::new(vec![None]) };
        assert_eq!(get_current_hostname(&runner), None);
    }

    #[test]
    fn set_hostname_success() {
        let dir = fixture();
        write_hosts(dir.path(), "127.0.0.1\tlocalhost oldhost\n");
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "oldhost\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
            ]),
        };
        assert!(set_hostname_with_hosts_update(&runner, dir.path(), "newhost"));
    }

    #[test]
    fn set_hostname_command_failure_returns_false() {
        let dir = fixture();
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "oldhost\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 1, stdout: String::new(), stderr: "Command failed".to_string() }),
            ]),
        };
        assert!(!set_hostname_with_hosts_update(&runner, dir.path(), "newhost"));
    }

    #[test]
    fn set_hostname_hosts_update_failure_not_critical() {
        // No etc/hosts in the fixture dir, so update_hosts_file's write will
        // fail (backup read fails); set_hostname_with_hosts_update must
        // still report success since the hostname itself was set.
        let dir = fixture();
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "oldhost\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
            ]),
        };
        assert!(set_hostname_with_hosts_update(&runner, dir.path(), "newhost"));
    }
}
