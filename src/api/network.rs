//! Port of `configurator/network.py` (physical network interface discovery
//! and DHCP/static-IP/IPv6 configuration via NetworkManager).
//!
//! Process access is abstracted behind [`CommandRunner`] and interface
//! enumeration behind [`NetworkInterfaces`] (mirroring the Python code's
//! `netifaces` dependency), so both can be exercised in tests. The
//! `is_physical_interface` classifier is also injectable as a plain
//! predicate wherever it's used, matching how the Python tests patch it.
//! Kernel-cmdline IPv6 persistence (`CmdlineTxt` in the original) has not
//! been ported, so `enable_ipv6`/`disable_ipv6` only manage sysctl files and
//! NetworkManager connections here.
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use serde::Serialize;

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

/// Real implementation that spawns processes via [`std::process::Command`].
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterfaceAddrs {
    pub mac: Option<String>,
    pub ipv4: Option<String>,
    pub netmask: Option<String>,
}

/// Local network interface enumeration abstraction (mirrors the Python
/// code's `netifaces` dependency).
pub trait NetworkInterfaces: Send + Sync {
    fn interfaces(&self) -> Vec<String>;
    /// `Err` mirrors a per-interface exception from `netifaces.ifaddresses`,
    /// which callers tolerate by skipping that interface.
    fn addresses(&self, interface: &str) -> Result<InterfaceAddrs, String>;
}

/// Real implementation that parses `ip -o addr show` / `ip -o link show`.
pub struct SystemNetworkInterfaces<'a> {
    pub runner: &'a dyn CommandRunner,
}

impl NetworkInterfaces for SystemNetworkInterfaces<'_> {
    fn interfaces(&self) -> Vec<String> {
        let output = self.runner.run(&["ip", "-o", "link", "show"]).map(|o| o.stdout).unwrap_or_default();
        output
            .lines()
            .filter_map(|line| {
                let rest = line.split_once(':')?.1;
                rest.split_whitespace().next().map(|s| s.trim_end_matches(':').to_string())
            })
            .collect()
    }

    fn addresses(&self, interface: &str) -> Result<InterfaceAddrs, String> {
        let mut info = InterfaceAddrs::default();

        if let Some(out) = self.runner.run(&["ip", "-o", "link", "show", interface]) {
            if out.success() {
                if let Some(pos) = out.stdout.find("link/ether") {
                    info.mac = out.stdout[pos + "link/ether".len()..].split_whitespace().next().map(|s| s.to_string());
                }
            }
        }

        if let Some(out) = self.runner.run(&["ip", "-o", "-4", "addr", "show", interface]) {
            if out.success() {
                if let Some(inet_pos) = out.stdout.find("inet ") {
                    let rest = &out.stdout[inet_pos + "inet ".len()..];
                    if let Some(cidr) = rest.split_whitespace().next() {
                        if let Some((ip, prefix)) = cidr.split_once('/') {
                            info.ipv4 = Some(ip.to_string());
                            if let Ok(prefix_len) = prefix.parse::<u32>() {
                                let mask = if prefix_len == 0 { 0 } else { u32::MAX << (32 - prefix_len) };
                                info.netmask = Some(Ipv4Addr::from(mask).to_string());
                            }
                        }
                    }
                }
            }
        }

        Ok(info)
    }
}

/// Return `true` if `interface` matches common wireless naming schemes.
pub fn is_wireless_interface_name(interface: &str) -> bool {
    interface.starts_with("wlan")
        || interface.starts_with("wlp")
        || interface.starts_with("wls")
        || interface.starts_with("wifi")
        || interface.starts_with("Wi-Fi")
}

/// Validate IPv4 CIDR notation such as `192.168.1.10/24`.
pub fn is_valid_ipv4_with_mask(ip_with_mask: &str) -> bool {
    let Some((ip, prefix)) = ip_with_mask.split_once('/') else {
        return false;
    };
    let Ok(prefix_len) = prefix.parse::<u32>() else {
        return false;
    };
    prefix_len <= 32 && Ipv4Addr::from_str(ip).is_ok()
}

/// Validate a plain IPv4 address such as `192.168.1.1`.
pub fn is_valid_ipv4_address(address: &str) -> bool {
    Ipv4Addr::from_str(address).is_ok()
}

const NON_PHYSICAL_PREFIXES: &[&str] = &["lo", "docker", "br-", "veth", "tun", "tap", "virbr", "vnet", "bond", "dummy"];
const WIRELESS_DRIVERS: &[&str] = &["iwlwifi", "ath9k", "ath10k", "brcmfmac", "rtl8192", "wl"];

/// Determine if `interface` is a physical interface (Ethernet or WiFi).
pub fn is_physical_interface(interface: &str, runner: &dyn CommandRunner, root: &Path) -> bool {
    if NON_PHYSICAL_PREFIXES.iter().any(|p| interface.starts_with(p)) {
        return false;
    }

    let is_wireless_by_proc = std::fs::read_to_string(root.join("proc/net/wireless")).map(|content| content.contains(interface)).unwrap_or(false);

    let is_ethernet = root.join(format!("sys/class/net/{interface}/device")).exists();

    let driver = runner.run(&["ethtool", "-i", interface]).filter(|out| out.success()).and_then(|out| {
        out.stdout.lines().find_map(|line| line.strip_prefix("driver:").map(|d| d.trim().to_string()))
    });

    let is_wireless_by_driver = driver.is_some_and(|d| WIRELESS_DRIVERS.iter().any(|w| d.contains(w)));

    if is_wireless_by_proc || is_wireless_by_driver || is_ethernet {
        return true;
    }

    is_ethernet_like_name(interface) || is_wifi_like_name(interface) || interface.starts_with("Ethernet") || interface.starts_with("Local Area Connection") || interface.starts_with("Wi-Fi")
}

fn is_ethernet_like_name(interface: &str) -> bool {
    let patterns = [r"^eth\d+$", r"^en[ospx]\d+$", r"^ens\d+$", r"^enp\d+s\d+$"];
    patterns.iter().any(|p| regex::Regex::new(p).unwrap().is_match(interface))
}

fn is_wifi_like_name(interface: &str) -> bool {
    let patterns = [r"^wlan\d+$", r"^wlp\d+s\d+$", r"^wls\d+$", r"^wifi\d+$"];
    patterns.iter().any(|p| regex::Regex::new(p).unwrap().is_match(interface))
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac: Option<String>,
    pub ipv4: Option<String>,
    pub netmask: Option<String>,
    pub state: String,
    #[serde(rename = "type")]
    pub interface_type: String,
}

fn read_operstate(root: &Path, interface: &str) -> Option<String> {
    std::fs::read_to_string(root.join(format!("sys/class/net/{interface}/operstate"))).ok().map(|s| s.trim().to_string())
}

/// List physical network interfaces (Ethernet and WiFi), with `is_physical`
/// injectable so callers/tests can substitute their own classifier.
pub fn list_physical_interfaces(nics: &dyn NetworkInterfaces, is_physical: &dyn Fn(&str) -> bool, root: &Path) -> Vec<InterfaceInfo> {
    let mut result = Vec::new();
    for interface in nics.interfaces() {
        if !is_physical(&interface) {
            continue;
        }
        let Ok(addrs) = nics.addresses(&interface) else {
            continue;
        };

        let state = read_operstate(root, &interface).unwrap_or_else(|| if addrs.ipv4.is_some() { "up".to_string() } else { "unknown".to_string() });
        let interface_type = if is_wireless_interface_name(&interface) { "wireless" } else { "wired" };

        result.push(InterfaceInfo { name: interface, mac: addrs.mac, ipv4: addrs.ipv4, netmask: addrs.netmask, state, interface_type: interface_type.to_string() });
    }
    result
}

fn active_connection_name(runner: &dyn CommandRunner, interface: &str) -> Option<String> {
    let output = runner.run(&["nmcli", "-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])?;
    if !output.success() {
        return None;
    }
    output.stdout.lines().find_map(|line| {
        let (name, device) = line.split_once(':')?;
        (device == interface).then(|| name.to_string())
    })
}

fn network_manager_is_active(runner: &dyn CommandRunner) -> bool {
    runner.run(&["systemctl", "is-active", "NetworkManager"]).is_some_and(|out| out.success())
}

/// Configure the specified interface to use DHCP via NetworkManager.
pub fn configure_dhcp(runner: &dyn CommandRunner, interface: &str, is_physical: &dyn Fn(&str) -> bool) -> bool {
    if !network_manager_is_active(runner) {
        return false;
    }
    if !is_physical(interface) {
        return false;
    }

    match active_connection_name(runner, interface) {
        Some(connection_name) => {
            let modify = runner.run(&["nmcli", "connection", "modify", &connection_name, "ipv4.method", "auto", "ipv4.addresses", "", "ipv4.gateway", ""]);
            if !modify.is_some_and(|o| o.success()) {
                return false;
            }
            runner.run(&["nmcli", "connection", "up", &connection_name]).is_some_and(|o| o.success())
        }
        None => {
            if is_wireless_interface_name(interface) {
                return false;
            }
            let connection_name = format!("dhcp-{interface}");
            let add = runner.run(&["nmcli", "connection", "add", "type", "ethernet", "con-name", &connection_name, "ifname", interface, "ipv4.method", "auto"]);
            if !add.is_some_and(|o| o.success()) {
                return false;
            }
            runner.run(&["nmcli", "connection", "up", &connection_name]).is_some_and(|o| o.success())
        }
    }
}

/// Configure the specified interface to use a static IPv4 address.
pub fn configure_fixed_ip(runner: &dyn CommandRunner, interface: &str, ip_with_mask: &str, router: &str, is_physical: &dyn Fn(&str) -> bool) -> bool {
    if !network_manager_is_active(runner) {
        return false;
    }
    if !is_physical(interface) {
        return false;
    }
    if !is_valid_ipv4_with_mask(ip_with_mask) {
        return false;
    }
    if !is_valid_ipv4_address(router) {
        return false;
    }

    match active_connection_name(runner, interface) {
        Some(connection_name) => {
            let modify = runner.run(&["nmcli", "connection", "modify", &connection_name, "ipv4.method", "manual", "ipv4.addresses", ip_with_mask, "ipv4.gateway", router]);
            if !modify.is_some_and(|o| o.success()) {
                return false;
            }
            runner.run(&["nmcli", "connection", "up", &connection_name]).is_some_and(|o| o.success())
        }
        None => {
            if is_wireless_interface_name(interface) {
                return false;
            }
            let connection_name = format!("static-{interface}");
            let add = runner.run(&[
                "nmcli",
                "connection",
                "add",
                "type",
                "ethernet",
                "con-name",
                &connection_name,
                "ifname",
                interface,
                "ipv4.method",
                "manual",
                "ipv4.addresses",
                ip_with_mask,
                "ipv4.gateway",
                router,
            ]);
            if !add.is_some_and(|o| o.success()) {
                return false;
            }
            runner.run(&["nmcli", "connection", "up", &connection_name]).is_some_and(|o| o.success())
        }
    }
}

fn active_connections(runner: &dyn CommandRunner) -> Vec<String> {
    runner
        .run(&["nmcli", "-t", "-f", "NAME", "connection", "show"])
        .filter(|o| o.success())
        .map(|o| o.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// Enable IPv6 system-wide via sysctl and NetworkManager connections.
///
/// Kernel command-line persistence (`CmdlineTxt` in the Python original) is
/// not ported; only the sysctl file and NetworkManager connection changes
/// are applied here.
pub fn enable_ipv6(runner: &dyn CommandRunner, sysctl_dir: &Path) -> bool {
    let disable_file = sysctl_dir.join("99-disable-ipv6.conf");
    if disable_file.exists() {
        if std::fs::remove_file(&disable_file).is_err() {
            return false;
        }
    }

    let enable_file = sysctl_dir.join("99-enable-ipv6.conf");
    let content = "# Enable IPv6\nnet.ipv6.conf.all.disable_ipv6 = 0\nnet.ipv6.conf.default.disable_ipv6 = 0\nnet.ipv6.conf.lo.disable_ipv6 = 0\n";
    if std::fs::write(&enable_file, content).is_err() {
        return false;
    }

    if !runner.run(&["sysctl", "-p", enable_file.to_str().unwrap_or_default()]).is_some_and(|o| o.success()) {
        return false;
    }

    let mut success = true;
    for connection in active_connections(runner) {
        if !runner.run(&["nmcli", "connection", "modify", &connection, "ipv6.method", "auto"]).is_some_and(|o| o.success()) {
            success = false;
        }
    }

    // Only restart if NetworkManager is actually active; avoids an
    // unconditional privileged restart attempt (and any resulting polkit
    // authentication prompt) when it's already stopped or absent.
    if network_manager_is_active(runner) {
        runner.run(&["systemctl", "restart", "NetworkManager"]);
    }
    success
}

/// Disable IPv6 system-wide via sysctl and NetworkManager connections.
///
/// Kernel command-line persistence (`CmdlineTxt` in the Python original) is
/// not ported; a reboot-required kernel-level disable is not applied here.
pub fn disable_ipv6(runner: &dyn CommandRunner, sysctl_dir: &Path) -> bool {
    let disable_file = sysctl_dir.join("99-disable-ipv6.conf");
    let content = "# Disable IPv6\nnet.ipv6.conf.all.disable_ipv6 = 1\nnet.ipv6.conf.default.disable_ipv6 = 1\nnet.ipv6.conf.lo.disable_ipv6 = 1\n";
    if std::fs::write(&disable_file, content).is_err() {
        return false;
    }

    let enable_file = sysctl_dir.join("99-enable-ipv6.conf");
    if enable_file.exists() {
        let _ = std::fs::remove_file(&enable_file);
    }

    if !runner.run(&["sysctl", "-p", disable_file.to_str().unwrap_or_default()]).is_some_and(|o| o.success()) {
        return false;
    }

    let mut success = true;
    for connection in active_connections(runner) {
        if !runner.run(&["nmcli", "connection", "modify", &connection, "ipv6.method", "disabled"]).is_some_and(|o| o.success()) {
            success = false;
        }
    }

    // Only restart if NetworkManager is actually active; avoids an
    // unconditional privileged restart attempt (and any resulting polkit
    // authentication prompt) when it's already stopped or absent.
    if network_manager_is_active(runner) {
        runner.run(&["systemctl", "restart", "NetworkManager"]);
    }
    success
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct NetworkConfig {
    pub hostname: String,
    pub default_gateway: Option<String>,
    pub dns_servers: Vec<String>,
    pub interfaces: Vec<InterfaceInfo>,
}

fn read_default_gateway(runner: &dyn CommandRunner) -> Option<String> {
    let output = runner.run(&["ip", "route", "show", "default"])?;
    if !output.success() {
        return None;
    }
    output.stdout.split_whitespace().collect::<Vec<_>>().windows(2).find(|w| w[0] == "via").map(|w| w[1].to_string())
}

fn read_dns_servers(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("etc/resolv.conf"))
        .map(|content| {
            content
                .lines()
                .filter_map(|line| line.trim().strip_prefix("nameserver"))
                .filter_map(|rest| rest.split_whitespace().next())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn read_hostname(root: &Path) -> String {
    std::fs::read_to_string(root.join("etc/hostname")).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Get network configuration: hostname, default gateway, DNS servers and physical interfaces.
pub fn get_network_config(nics: &dyn NetworkInterfaces, is_physical: &dyn Fn(&str) -> bool, runner: &dyn CommandRunner, root: &Path) -> NetworkConfig {
    NetworkConfig {
        hostname: read_hostname(root),
        default_gateway: read_default_gateway(runner),
        dns_servers: read_dns_servers(root),
        interfaces: list_physical_interfaces(nics, is_physical, root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FakeNics {
        interfaces: Vec<String>,
        addrs: HashMap<String, Result<InterfaceAddrs, String>>,
    }

    impl NetworkInterfaces for FakeNics {
        fn interfaces(&self) -> Vec<String> {
            self.interfaces.clone()
        }
        fn addresses(&self, interface: &str) -> Result<InterfaceAddrs, String> {
            self.addrs.get(interface).cloned().unwrap_or(Ok(InterfaceAddrs::default()))
        }
    }

    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn list_physical_interfaces_skips_interface_address_errors() {
        let mut addrs = HashMap::new();
        addrs.insert("eth0".to_string(), Err("transient failure".to_string()));
        addrs.insert(
            "eth1".to_string(),
            Ok(InterfaceAddrs { mac: Some("aa:bb:cc:dd:ee:ff".to_string()), ipv4: Some("192.168.1.10".to_string()), netmask: Some("255.255.255.0".to_string()) }),
        );
        let nics = FakeNics { interfaces: vec!["eth0".to_string(), "eth1".to_string()], addrs };

        let dir = fixture();
        write(dir.path(), "sys/class/net/eth1/operstate", "up\n");

        let result = list_physical_interfaces(&nics, &|_| true, dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "eth1");
        assert_eq!(result[0].state, "up");
    }

    #[test]
    fn list_physical_interfaces_handles_no_interfaces() {
        let nics = FakeNics { interfaces: Vec::new(), addrs: HashMap::new() };
        let dir = fixture();
        assert_eq!(list_physical_interfaces(&nics, &|_| true, dir.path()), Vec::new());
    }

    struct StubRunner {
        responses: Mutex<Vec<CommandOutput>>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, _args: &[&str]) -> Option<CommandOutput> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                None
            } else {
                Some(responses.remove(0))
            }
        }
    }

    #[test]
    fn configure_fixed_ip_rejects_semantically_invalid_ipv4() {
        let runner = StubRunner { responses: Mutex::new(vec![CommandOutput { status: 0, stdout: "active\n".to_string(), stderr: String::new() }]) };
        assert!(!configure_fixed_ip(&runner, "eth0", "999.1.1.1/24", "192.168.1.1", &|_| true));

        let runner = StubRunner { responses: Mutex::new(vec![CommandOutput { status: 0, stdout: "active\n".to_string(), stderr: String::new() }]) };
        assert!(!configure_fixed_ip(&runner, "eth0", "192.168.1.10/24", "300.1.1.1", &|_| true));
    }

    #[test]
    fn configure_dhcp_wireless_without_active_connection_fails_cleanly() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                CommandOutput { status: 0, stdout: "active\n".to_string(), stderr: String::new() },
                CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
            ]),
        };
        assert!(!configure_dhcp(&runner, "wlan0", &|_| true));
    }

    #[test]
    fn configure_fixed_ip_wireless_without_active_connection_fails_cleanly() {
        let runner = StubRunner {
            responses: Mutex::new(vec![
                CommandOutput { status: 0, stdout: "active\n".to_string(), stderr: String::new() },
                CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
            ]),
        };
        assert!(!configure_fixed_ip(&runner, "wlan0", "192.168.1.10/24", "192.168.1.1", &|_| true));
    }

    #[test]
    fn get_network_config_without_interfaces() {
        let nics = FakeNics { interfaces: Vec::new(), addrs: HashMap::new() };
        let runner = StubRunner { responses: Mutex::new(Vec::new()) };
        let dir = fixture();
        write(dir.path(), "etc/resolv.conf", "nameserver 1.1.1.1\n");
        write(dir.path(), "etc/hostname", "myhost\n");

        let config = get_network_config(&nics, &|_| true, &runner, dir.path());
        assert_eq!(config.hostname, "myhost");
        assert_eq!(config.default_gateway, None);
        assert_eq!(config.dns_servers, vec!["1.1.1.1".to_string()]);
        assert!(config.interfaces.is_empty());
    }

    #[test]
    fn is_valid_ipv4_with_mask_accepts_and_rejects() {
        assert!(is_valid_ipv4_with_mask("192.168.1.10/24"));
        assert!(!is_valid_ipv4_with_mask("999.1.1.1/24"));
        assert!(!is_valid_ipv4_with_mask("192.168.1.10/33"));
        assert!(!is_valid_ipv4_with_mask("192.168.1.10"));
    }

    #[test]
    fn is_valid_ipv4_address_accepts_and_rejects() {
        assert!(is_valid_ipv4_address("192.168.1.1"));
        assert!(!is_valid_ipv4_address("300.1.1.1"));
    }

    #[test]
    fn is_wireless_interface_name_matches_common_schemes() {
        assert!(is_wireless_interface_name("wlan0"));
        assert!(is_wireless_interface_name("wlp3s0"));
        assert!(!is_wireless_interface_name("eth0"));
    }

    #[test]
    fn is_physical_interface_rejects_virtual_prefixes() {
        let runner = StubRunner { responses: Mutex::new(Vec::new()) };
        let dir = fixture();
        assert!(!is_physical_interface("lo", &runner, dir.path()));
        assert!(!is_physical_interface("docker0", &runner, dir.path()));
        assert!(!is_physical_interface("veth123", &runner, dir.path()));
    }

    #[test]
    fn is_physical_interface_detects_ethernet_device() {
        let runner = StubRunner { responses: Mutex::new(Vec::new()) };
        let dir = fixture();
        write(dir.path(), "sys/class/net/eth0/device", "");
        assert!(is_physical_interface("eth0", &runner, dir.path()));
    }

    struct RecordingRunner {
        network_manager_active: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, args: &[&str]) -> Option<CommandOutput> {
            self.calls.lock().unwrap().push(args.iter().map(|s| s.to_string()).collect());
            match args {
                ["systemctl", "is-active", "NetworkManager"] => {
                    Some(CommandOutput { status: if self.network_manager_active { 0 } else { 3 }, stdout: String::new(), stderr: String::new() })
                }
                ["nmcli", "-t", "-f", "NAME", "connection", "show"] => Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
                _ => Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
            }
        }
    }

    #[test]
    fn enable_ipv6_skips_restart_when_network_manager_inactive() {
        let runner = RecordingRunner { network_manager_active: false, calls: Mutex::new(Vec::new()) };
        let dir = fixture();
        assert!(enable_ipv6(&runner, dir.path()));
        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c == &vec!["systemctl".to_string(), "restart".to_string(), "NetworkManager".to_string()]));
    }

    #[test]
    fn enable_ipv6_restarts_when_network_manager_active() {
        let runner = RecordingRunner { network_manager_active: true, calls: Mutex::new(Vec::new()) };
        let dir = fixture();
        assert!(enable_ipv6(&runner, dir.path()));
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|c| c == &vec!["systemctl".to_string(), "restart".to_string(), "NetworkManager".to_string()]));
    }

    #[test]
    fn disable_ipv6_skips_restart_when_network_manager_inactive() {
        let runner = RecordingRunner { network_manager_active: false, calls: Mutex::new(Vec::new()) };
        let dir = fixture();
        assert!(disable_ipv6(&runner, dir.path()));
        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c == &vec!["systemctl".to_string(), "restart".to_string(), "NetworkManager".to_string()]));
    }
}
