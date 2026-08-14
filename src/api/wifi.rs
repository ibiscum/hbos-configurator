//! Port of `configurator/wifi.py` (NetworkManager/iw-based WiFi management).
//!
//! Subprocess access is abstracted behind [`CommandRunner`] so the control
//! flow can be exercised in tests the same way Python patches `subprocess.run`.
use std::collections::VecDeque;
use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn new(status: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    pub fn success(&self) -> bool {
        self.status == 0
    }
}

/// Runs external commands. `None` mirrors Python's `FileNotFoundError`/`SubprocessError`.
pub trait CommandRunner {
    fn run(&self, args: &[&str]) -> Option<CommandOutput>;
}

/// Real implementation that spawns processes via [`std::process::Command`].
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, args: &[&str]) -> Option<CommandOutput> {
        let (program, rest) = args.split_first()?;
        match Command::new(program).args(rest).output() {
            Ok(out) => Some(CommandOutput {
                status: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            }),
            Err(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: i32,
    pub security: String,
    pub channel: String,
    pub bssid: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WifiConnection {
    pub name: String,
    pub device: String,
    pub ssid: String,
    pub ip: Option<String>,
    pub security: String,
}

/// Split nmcli terse (`-t`) output on unescaped colons.
pub fn split_nmcli_terse_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            ':' => {
                fields.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        current.push('\\');
    }
    fields.push(current);
    fields
}

/// Whether an nmcli connection "TYPE" value represents a WiFi connection.
pub fn is_wifi_connection_type(connection_type: &str) -> bool {
    matches!(connection_type, "wifi" | "802-11-wireless")
}

/// Convert a WiFi channel frequency (MHz) to a channel number.
pub fn freq_to_channel(freq: i32) -> i32 {
    if freq == 2484 {
        14
    } else if (2412..=2472).contains(&freq) {
        (freq - 2407) / 5
    } else if freq >= 5000 {
        (freq - 5000) / 5
    } else {
        0
    }
}

/// Convert a dBm signal reading to an approximate percentage (0-100).
pub fn dbm_to_percent(dbm: f64) -> i32 {
    (((dbm + 100.0) * 2.0).round() as i32).clamp(0, 100)
}

/// Parse `iw dev <iface> scan` output into a list of networks.
pub fn parse_iw_scan_output(output: &str) -> Vec<WifiNetwork> {
    let mut networks = Vec::new();
    let mut current: Option<WifiNetwork> = None;

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("BSS ") {
            if let Some(n) = current.take() {
                if !n.ssid.is_empty() {
                    networks.push(n);
                }
            }
            let bssid = rest.split('(').next().unwrap_or("").trim().to_string();
            current = Some(WifiNetwork {
                bssid,
                ..Default::default()
            });
        } else if let Some(rest) = line.strip_prefix("SSID: ") {
            if let Some(n) = current.as_mut() {
                if !rest.is_empty() {
                    n.ssid = rest.to_string();
                }
            }
        } else if let Some(rest) = line.strip_prefix("signal: ") {
            if let Some(n) = current.as_mut() {
                let raw = rest.split(' ').next().unwrap_or("0");
                n.signal = raw.parse::<f64>().map(dbm_to_percent).unwrap_or(0);
            }
        } else if let Some(rest) = line.strip_prefix("freq: ") {
            if let Some(n) = current.as_mut() {
                n.channel = rest
                    .trim()
                    .parse::<i32>()
                    .map(|f| freq_to_channel(f).to_string())
                    .unwrap_or_default();
            }
        } else if line.contains("capability: ") {
            if let Some(n) = current.as_mut() {
                n.security = if line.contains("Privacy") {
                    "Protected".to_string()
                } else {
                    "Open".to_string()
                };
            }
        } else if line.contains("RSN") || line.contains("WPA") {
            if let Some(n) = current.as_mut() {
                n.security = "WPA".to_string();
            }
        }
    }

    if let Some(n) = current {
        if !n.ssid.is_empty() {
            networks.push(n);
        }
    }
    networks
}

/// Parse `nmcli -t -f SSID,SIGNAL,SECURITY,CHAN,BSSID,BARS device wifi list` output.
pub fn parse_nmcli_scan_output(output: &str) -> Vec<WifiNetwork> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_nmcli_terse_line(line);
            if fields.len() < 5 || fields[0].is_empty() {
                return None;
            }
            Some(WifiNetwork {
                ssid: fields[0].clone(),
                signal: fields[1].parse::<i32>().unwrap_or(0),
                security: if fields[2].is_empty() {
                    "Open".to_string()
                } else {
                    fields[2].clone()
                },
                channel: fields.get(3).cloned().unwrap_or_default(),
                bssid: fields.get(4).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// Find available wireless interface names (`iw dev`, falling back to `nmcli`).
pub fn find_wireless_interfaces(runner: &dyn CommandRunner) -> Vec<String> {
    if let Some(out) = runner.run(&["iw", "dev"]) {
        if out.success() {
            let interfaces: Vec<String> = out
                .stdout
                .lines()
                .filter_map(|l| l.split_once("Interface").map(|(_, rest)| rest.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            if !interfaces.is_empty() {
                return interfaces;
            }
        }
    }

    if let Some(out) = runner.run(&["nmcli", "-t", "-f", "DEVICE,TYPE", "device"]) {
        if out.success() {
            let interfaces: Vec<String> = out
                .stdout
                .lines()
                .filter(|l| l.contains(":wifi"))
                .filter_map(|l| l.split(':').next().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            if !interfaces.is_empty() {
                return interfaces;
            }
        }
    }

    Vec::new()
}

fn scan_with_networkmanager(runner: &dyn CommandRunner, interface: &str) -> Vec<WifiNetwork> {
    let _ = runner.run(&["nmcli", "device", "wifi", "rescan", "ifname", interface]);
    match runner.run(&[
        "nmcli",
        "-t",
        "-f",
        "SSID,SIGNAL,SECURITY,CHAN,BSSID,BARS",
        "device",
        "wifi",
        "list",
        "ifname",
        interface,
    ]) {
        Some(out) if out.success() => parse_nmcli_scan_output(&out.stdout),
        _ => Vec::new(),
    }
}

fn scan_with_iw(runner: &dyn CommandRunner, interface: &str) -> Vec<WifiNetwork> {
    match runner.run(&["iw", "dev", interface, "scan"]) {
        Some(out) if out.success() => parse_iw_scan_output(&out.stdout),
        _ => Vec::new(),
    }
}

/// Scan for WiFi networks, preferring NetworkManager and falling back to `iw`.
pub fn scan_wifi_networks(runner: &dyn CommandRunner) -> Vec<WifiNetwork> {
    let interfaces = find_wireless_interfaces(runner);
    let Some(interface) = interfaces.first() else {
        return Vec::new();
    };

    let nm_active = runner
        .run(&["systemctl", "is-active", "NetworkManager"])
        .map(|o| o.success())
        .unwrap_or(false);

    let mut networks = if nm_active {
        scan_with_networkmanager(runner, interface)
    } else {
        scan_with_iw(runner, interface)
    };
    networks.sort_by(|a, b| b.signal.cmp(&a.signal));
    networks
}

/// Save the currently active WiFi connection, if any.
pub fn save_current_connection(runner: &dyn CommandRunner) -> Option<WifiConnection> {
    let out = runner.run(&["nmcli", "-t", "-f", "NAME,DEVICE,TYPE", "connection", "show", "--active"])?;
    if !out.success() {
        return None;
    }
    for line in out.stdout.lines() {
        let fields = split_nmcli_terse_line(line);
        if fields.len() >= 3 && is_wifi_connection_type(&fields[2]) {
            return Some(WifiConnection {
                name: fields[0].clone(),
                device: fields[1].clone(),
                ssid: fields[0].clone(),
                ip: None,
                security: "Unknown".to_string(),
            });
        }
    }
    None
}

/// Get details about the currently active WiFi connection, if any.
pub fn get_current_connection(runner: &dyn CommandRunner) -> Option<WifiConnection> {
    let out = runner.run(&["nmcli", "-t", "-f", "NAME,DEVICE,TYPE,ACTIVE", "connection", "show"])?;
    if !out.success() {
        return None;
    }

    for line in out.stdout.lines() {
        let is_active_wifi = (line.contains(":802-11-wireless:") || line.contains(":wifi:")) && line.contains(":yes");
        if !is_active_wifi {
            continue;
        }
        let fields = split_nmcli_terse_line(line);
        if fields.len() < 3 {
            continue;
        }
        let connection_name = fields[0].clone();
        let device = fields[1].clone();

        let details = runner.run(&["nmcli", "-t", "connection", "show", &connection_name])?;
        if !details.success() {
            continue;
        }

        let mut ssid = connection_name.clone();
        for detail in details.stdout.lines() {
            if let Some(v) = detail.strip_prefix("802-11-wireless.ssid:") {
                if !v.trim().is_empty() {
                    ssid = v.trim().to_string();
                    break;
                }
            }
        }

        let mut security = "Unknown".to_string();
        for detail in details.stdout.lines() {
            if let Some(v) = detail.strip_prefix("802-11-wireless-security.key-mgmt:") {
                if !v.trim().is_empty() {
                    security = v.trim().to_string();
                }
                break;
            }
        }

        let ip = runner
            .run(&["nmcli", "-t", "-f", "IP4.ADDRESS", "connection", "show", &connection_name])
            .filter(|r| r.success())
            .and_then(|r| {
                r.stdout
                    .lines()
                    .find_map(|l| l.strip_prefix("IP4.ADDRESS").map(|v| v.trim_start_matches(':').to_string()))
            })
            .and_then(|v| v.split('/').next().map(|s| s.to_string()))
            .filter(|s| !s.is_empty());

        if ssid == connection_name {
            if let Some(iw) = runner.run(&["iwconfig", &device]) {
                if iw.success() {
                    for l in iw.stdout.lines() {
                        if let Some(v) = l.split("ESSID:").nth(1) {
                            let essid = v.trim().trim_matches('"');
                            if !essid.is_empty() && essid != "off/any" {
                                ssid = essid.to_string();
                                break;
                            }
                        }
                    }
                }
            }
        }

        return Some(WifiConnection {
            name: connection_name,
            device,
            ssid,
            ip,
            security,
        });
    }
    None
}

fn handle_connection_failure(runner: &dyn CommandRunner, old: Option<&WifiConnection>, revert: bool) -> bool {
    if revert {
        if let Some(old) = old {
            let _ = runner.run(&["nmcli", "connection", "up", &old.name]);
        }
    }
    false
}

/// Connect to a WiFi network, optionally reverting to the previous connection on failure.
pub fn connect_to_wifi(
    runner: &dyn CommandRunner,
    ssid: &str,
    passphrase: Option<&str>,
    revert_on_failure: bool,
) -> bool {
    match runner.run(&["systemctl", "is-active", "NetworkManager"]) {
        Some(out) if out.success() => {}
        _ => return false,
    }

    let interfaces = find_wireless_interfaces(runner);
    let Some(interface) = interfaces.first() else {
        return false;
    };

    let old_connection = if revert_on_failure {
        save_current_connection(runner)
    } else {
        None
    };

    let existing = runner
        .run(&["nmcli", "-t", "-f", "NAME", "connection", "show"])
        .filter(|o| o.success())
        .and_then(|o| {
            o.stdout
                .lines()
                .filter_map(|l| split_nmcli_terse_line(l).into_iter().next())
                .find(|name| name == ssid)
        });

    let connect_ok = if let Some(name) = &existing {
        runner
            .run(&["nmcli", "connection", "up", name])
            .map(|o| o.success())
            .unwrap_or(false)
    } else if let Some(pass) = passphrase {
        runner
            .run(&["nmcli", "device", "wifi", "connect", ssid, "password", pass, "ifname", interface])
            .map(|o| o.success())
            .unwrap_or(false)
    } else {
        runner
            .run(&["nmcli", "device", "wifi", "connect", ssid, "ifname", interface])
            .map(|o| o.success())
            .unwrap_or(false)
    };

    if !connect_ok {
        return handle_connection_failure(runner, old_connection.as_ref(), revert_on_failure);
    }

    let verified = runner
        .run(&["nmcli", "-t", "-f", "NAME,TYPE", "connection", "show", "--active"])
        .filter(|o| o.success())
        .map(|o| {
            o.stdout.lines().any(|l| {
                let fields = split_nmcli_terse_line(l);
                fields.len() >= 2 && is_wifi_connection_type(&fields[1]) && fields[0] == ssid
            })
        })
        .unwrap_or(false);

    if verified {
        true
    } else {
        handle_connection_failure(runner, old_connection.as_ref(), revert_on_failure)
    }
}

/// Test double that replays a fixed sequence of canned command results.
pub struct FakeCommandRunner {
    responses: Mutex<VecDeque<Option<CommandOutput>>>,
}

impl FakeCommandRunner {
    pub fn new(responses: Vec<Option<CommandOutput>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }
}

impl CommandRunner for FakeCommandRunner {
    fn run(&self, _args: &[&str]) -> Option<CommandOutput> {
        self.responses.lock().unwrap().pop_front().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(status: i32, stdout: &str, stderr: &str) -> Option<CommandOutput> {
        Some(CommandOutput::new(status, stdout, stderr))
    }

    #[test]
    fn split_nmcli_terse_line_handles_escaped_colons() {
        assert_eq!(
            split_nmcli_terse_line(r"My\:Network:wlan0:wifi"),
            vec!["My:Network".to_string(), "wlan0".to_string(), "wifi".to_string()]
        );
    }

    #[test]
    fn is_wifi_connection_type_accepts_both_spellings() {
        assert!(is_wifi_connection_type("wifi"));
        assert!(is_wifi_connection_type("802-11-wireless"));
        assert!(!is_wifi_connection_type("ethernet"));
    }

    #[test]
    fn freq_to_channel_maps_2484_to_channel_14() {
        assert_eq!(freq_to_channel(2484), 14);
        assert_eq!(freq_to_channel(2412), 1);
        assert_eq!(freq_to_channel(5180), 36);
    }

    #[test]
    fn scan_with_iw_maps_2484_to_channel_14() {
        let output = ["BSS 00:11:22:33:44:55(on wlan0)", "\tfreq: 2484", "\tsignal: -45.00 dBm", "\tSSID: ch14-net"]
            .join("\n");
        let networks = parse_iw_scan_output(&output);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].channel, "14");
        assert_eq!(networks[0].ssid, "ch14-net");
    }

    #[test]
    fn find_wireless_interfaces_from_iw() {
        let runner = FakeCommandRunner::new(vec![cp(0, "phy#0\n\tInterface wlan0\n", "")]);
        assert_eq!(find_wireless_interfaces(&runner), vec!["wlan0".to_string()]);
    }

    #[test]
    fn find_wireless_interfaces_fallback_nmcli() {
        let runner = FakeCommandRunner::new(vec![cp(1, "", ""), cp(0, "eth0:ethernet\nwlan0:wifi\n", "")]);
        assert_eq!(find_wireless_interfaces(&runner), vec!["wlan0".to_string()]);
    }

    #[test]
    fn scan_wifi_networks_no_interfaces_returns_empty() {
        let runner = FakeCommandRunner::new(vec![cp(1, "", ""), cp(1, "", "")]);
        assert!(scan_wifi_networks(&runner).is_empty());
    }

    #[test]
    fn scan_wifi_networks_uses_networkmanager_and_sorts_by_signal() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "phy#0\n\tInterface wlan0\n", ""), // find_wireless_interfaces (iw dev)
            cp(0, "active\n", ""),                   // systemctl is-active
            None,                                     // nmcli rescan
            cp(0, "weak:10:Open::\nstrong:90:Open::\n", ""),
        ]);
        let result = scan_wifi_networks(&runner);
        assert_eq!(result.iter().map(|n| n.ssid.clone()).collect::<Vec<_>>(), vec!["strong", "weak"]);
    }

    #[test]
    fn scan_wifi_networks_uses_iw_when_nm_inactive() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "phy#0\n\tInterface wlan0\n", ""),
            cp(1, "", ""), // systemctl is-active fails -> not active
            cp(0, "BSS aa:bb(on wlan0)\n\tSSID: x\n\tsignal: -70.00 dBm\n", ""),
        ]);
        let result = scan_wifi_networks(&runner);
        assert_eq!(result[0].ssid, "x");
    }

    #[test]
    fn get_current_connection_primary_success() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "Conn:wlan0:wifi:yes\n", ""),
            cp(0, "802-11-wireless.ssid:MyNet\n", ""),
            cp(0, "IP4.ADDRESS:10.0.0.5/24\n", ""),
        ]);
        let result = get_current_connection(&runner).unwrap();
        assert_eq!(result.ssid, "MyNet");
        assert_eq!(result.ip, Some("10.0.0.5".to_string()));
    }

    #[test]
    fn get_current_connection_uses_iwconfig_fallback() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "Conn:wlan0:wifi:yes\n", ""),
            cp(0, "802-11-wireless-security.key-mgmt:wpa-psk\n", ""),
            cp(1, "", ""),
            cp(0, "wlan0     IEEE 802.11  ESSID:\"Cafe\"\n", ""),
        ]);
        let result = get_current_connection(&runner).unwrap();
        assert_eq!(result.ssid, "Cafe");
    }

    #[test]
    fn get_current_connection_handles_missing_nmcli() {
        let runner = FakeCommandRunner::new(vec![None]);
        assert!(get_current_connection(&runner).is_none());
    }

    #[test]
    fn connect_to_wifi_fails_when_nm_inactive() {
        let runner = FakeCommandRunner::new(vec![cp(1, "inactive\n", "")]);
        assert!(!connect_to_wifi(&runner, "MyNet", None, false));
    }

    #[test]
    fn connect_to_wifi_fails_with_no_interfaces() {
        let runner = FakeCommandRunner::new(vec![cp(0, "active\n", ""), cp(1, "", ""), cp(1, "", "")]);
        assert!(!connect_to_wifi(&runner, "MyNet", None, false));
    }

    #[test]
    fn connect_to_wifi_open_network_success() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "active\n", ""),               // systemctl is-active
            cp(0, "phy#0\n\tInterface wlan0\n", ""), // find_wireless_interfaces
            cp(0, "OtherNet\n", ""),              // existing profiles list (no match)
            cp(0, "", ""),                        // nmcli device wifi connect
            cp(0, "MyNet:wifi\n", ""),            // verify active connections
        ]);
        assert!(connect_to_wifi(&runner, "MyNet", None, false));
    }

    #[test]
    fn connect_to_wifi_passphrase_success() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "active\n", ""),
            cp(0, "phy#0\n\tInterface wlan0\n", ""),
            cp(0, "OtherNet\n", ""),
            cp(0, "", ""),
            cp(0, "MyNet:wifi\n", ""),
        ]);
        assert!(connect_to_wifi(&runner, "MyNet", Some("secret"), false));
    }

    #[test]
    fn connect_to_wifi_connection_command_fails_without_revert() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "active\n", ""),
            cp(0, "phy#0\n\tInterface wlan0\n", ""),
            cp(0, "OtherNet\n", ""),
            cp(1, "", "failed"),
        ]);
        assert!(!connect_to_wifi(&runner, "MyNet", None, false));
    }

    #[test]
    fn connect_to_wifi_existing_profile_success() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "active\n", ""),
            cp(0, "phy#0\n\tInterface wlan0\n", ""),
            cp(0, "MyNet\n", ""), // existing profile match
            cp(0, "", ""),        // nmcli connection up
            cp(0, "MyNet:wifi\n", ""),
        ]);
        assert!(connect_to_wifi(&runner, "MyNet", None, false));
    }

    #[test]
    fn connect_to_wifi_reverts_to_previous_connection_on_failure() {
        let runner = FakeCommandRunner::new(vec![
            cp(0, "active\n", ""),                       // systemctl is-active
            cp(0, "phy#0\n\tInterface wlan0\n", ""),      // find_wireless_interfaces
            cp(0, "OldNet:wlan0:wifi\n", ""),             // save_current_connection
            cp(0, "OtherNet\n", ""),                      // existing profiles (no match)
            cp(1, "", "failed"),                          // connect attempt fails
            None,                                          // revert command (result ignored)
        ]);
        assert!(!connect_to_wifi(&runner, "MyNet", None, true));
    }

    #[test]
    fn dbm_to_percent_clamps_to_valid_range() {
        assert_eq!(dbm_to_percent(-100.0), 0);
        assert_eq!(dbm_to_percent(0.0), 100);
    }
}
