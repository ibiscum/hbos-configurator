//! Port of `configurator/hostname_utils.py`: hostname helpers shared by CLI
//! tools and API handlers, built on top of `hostconfig`.
use std::path::Path;

use crate::api::hostconfig::{self, CommandRunner};

/// Get current system hostname and pretty hostname using `hostnamectl`.
///
/// Returns `(None, None)` if the underlying commands can't be run at all
/// (mirrors the Python original's broad exception handling).
pub fn get_hostnames(runner: &dyn CommandRunner) -> (Option<String>, Option<String>) {
    let hostname = runner.run(&["hostnamectl", "hostname"]).filter(|o| o.success()).map(|o| o.stdout.trim().to_string());

    let pretty_hostname = runner.run(&["hostnamectl", "--pretty"]).filter(|o| o.success()).map(|o| o.stdout.trim().to_string());
    // Empty pretty hostname output means it's not set.
    let pretty_hostname = pretty_hostname.filter(|p| !p.is_empty());

    (hostname, pretty_hostname)
}

/// Get hostnames with fallback logic: if no pretty hostname is set, reuse the
/// plain hostname.
pub fn get_hostnames_with_fallback(runner: &dyn CommandRunner) -> (Option<String>, Option<String>) {
    let (hostname, pretty_hostname) = get_hostnames(runner);
    let pretty_hostname = pretty_hostname.or_else(|| hostname.clone());
    (hostname, pretty_hostname)
}

/// Convert a pretty hostname to a valid system hostname (max 64 chars).
pub fn sanitize_hostname(pretty_hostname: &str) -> String {
    hostconfig::sanitize_hostname(pretty_hostname, 64)
}

/// Validate system hostname format.
pub fn validate_hostname(hostname: &str) -> bool {
    hostconfig::validate_hostname(hostname)
}

/// Validate pretty hostname format: non-empty, printable ASCII, max 64 chars.
pub fn validate_pretty_hostname(pretty_hostname: &str) -> bool {
    if pretty_hostname.is_empty() {
        return false;
    }
    if pretty_hostname.trim().is_empty() {
        return false;
    }
    if pretty_hostname.len() > 64 {
        return false;
    }
    if !pretty_hostname.is_ascii() {
        return false;
    }
    // Mirrors Python's str.isprintable(): all chars in the printable ASCII range.
    pretty_hostname.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Set system hostname using `hostnamectl` and update `/etc/hosts`.
pub fn set_hostname(runner: &dyn CommandRunner, root: &Path, hostname: &str) -> bool {
    hostconfig::set_hostname_with_hosts_update(runner, root, hostname)
}

/// Set pretty hostname using `hostnamectl`.
pub fn set_pretty_hostname(runner: &dyn CommandRunner, pretty_hostname: &str) -> bool {
    runner.run(&["hostnamectl", "set-hostname", "--pretty", pretty_hostname]).is_some_and(|o| o.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::hostconfig::CommandOutput;
    use std::sync::Mutex;

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
    fn get_hostnames_success() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "hifiberry\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: "HiFiBerry Device\n".to_string(), stderr: String::new() }),
            ]),
        };
        let (hostname, pretty) = get_hostnames(&runner);
        assert_eq!(hostname, Some("hifiberry".to_string()));
        assert_eq!(pretty, Some("HiFiBerry Device".to_string()));
    }

    #[test]
    fn get_hostnames_pretty_empty_becomes_none() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "hifiberry\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: "\n".to_string(), stderr: String::new() }),
            ]),
        };
        let (hostname, pretty) = get_hostnames(&runner);
        assert_eq!(hostname, Some("hifiberry".to_string()));
        assert_eq!(pretty, None);
    }

    #[test]
    fn get_hostnames_failure_for_hostname_command() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 1, stdout: String::new(), stderr: "failed".to_string() }),
                Some(CommandOutput { status: 0, stdout: "Pretty Name\n".to_string(), stderr: String::new() }),
            ]),
        };
        let (hostname, pretty) = get_hostnames(&runner);
        assert_eq!(hostname, None);
        assert_eq!(pretty, Some("Pretty Name".to_string()));
    }

    #[test]
    fn get_hostnames_missing_binary_returns_none_none() {
        let runner = StubRunner { responses: Mutex::new(vec![None, None]) };
        assert_eq!(get_hostnames(&runner), (None, None));
    }

    #[test]
    fn validate_pretty_hostname_valid() {
        assert!(validate_pretty_hostname("HiFiBerry Device"));
    }

    #[test]
    fn validate_pretty_hostname_empty_rejected() {
        assert!(!validate_pretty_hostname(""));
    }

    #[test]
    fn validate_pretty_hostname_whitespace_only_rejected() {
        assert!(!validate_pretty_hostname("   "));
    }

    #[test]
    fn validate_pretty_hostname_too_long_rejected() {
        assert!(!validate_pretty_hostname(&"A".repeat(65)));
    }

    #[test]
    fn validate_pretty_hostname_non_ascii_rejected() {
        assert!(!validate_pretty_hostname("HiFi-音声"));
    }

    #[test]
    fn validate_pretty_hostname_non_printable_rejected() {
        assert!(!validate_pretty_hostname("HiFi\nBerry"));
    }

    #[test]
    fn set_pretty_hostname_success() {
        let runner = StubRunner { responses: Mutex::new(vec![Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() })]) };
        assert!(set_pretty_hostname(&runner, "My Device"));
    }

    #[test]
    fn set_pretty_hostname_failure() {
        let runner = StubRunner { responses: Mutex::new(vec![Some(CommandOutput { status: 1, stdout: String::new(), stderr: "failed".to_string() })]) };
        assert!(!set_pretty_hostname(&runner, "My Device"));
    }

    #[test]
    fn set_pretty_hostname_missing_binary_returns_false() {
        let runner = StubRunner { responses: Mutex::new(vec![None]) };
        assert!(!set_pretty_hostname(&runner, "My Device"));
    }

    #[test]
    fn get_hostnames_with_fallback_uses_hostname_when_pretty_missing() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "hifiberry\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: "\n".to_string(), stderr: String::new() }),
            ]),
        };
        let (hostname, pretty) = get_hostnames_with_fallback(&runner);
        assert_eq!(hostname, Some("hifiberry".to_string()));
        assert_eq!(pretty, Some("hifiberry".to_string()));
    }

    #[test]
    fn get_hostnames_with_fallback_preserves_existing_pretty() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                Some(CommandOutput { status: 0, stdout: "hifiberry\n".to_string(), stderr: String::new() }),
                Some(CommandOutput { status: 0, stdout: "HiFiBerry\n".to_string(), stderr: String::new() }),
            ]),
        };
        let (hostname, pretty) = get_hostnames_with_fallback(&runner);
        assert_eq!(hostname, Some("hifiberry".to_string()));
        assert_eq!(pretty, Some("HiFiBerry".to_string()));
    }
}
