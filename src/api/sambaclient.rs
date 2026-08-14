//! Port of `configurator/sambaclient.py` (SMB/CIFS network discovery and client tools).
//!
//! Process/network access is abstracted behind [`CommandRunner`] (for
//! `smbclient`/`nmblookup`) and [`NetworkInterfaces`] (for local interface
//! enumeration, mirroring the Python code's `netifaces` dependency) so both
//! can be exercised in tests. `SystemNetworkInterfaces` discovers interfaces
//! by parsing `ip -o -4 addr show` output rather than adding a new native
//! dependency.
use std::net::Ipv4Addr;
use std::process::Command;
use std::str::FromStr;

use regex::Regex;
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ipv4AddrInfo {
    pub addr: Option<String>,
    pub netmask: Option<String>,
    pub broadcast: Option<String>,
}

/// Local network interface enumeration abstraction (mirrors the Python
/// code's `netifaces` dependency).
pub trait NetworkInterfaces: Send + Sync {
    fn interfaces(&self) -> Vec<String>;
    /// `Err` mirrors a per-interface `OSError`/`ValueError` from `netifaces`,
    /// which callers tolerate by skipping that interface.
    fn ipv4_addresses(&self, interface: &str) -> Result<Vec<Ipv4AddrInfo>, String>;
}

/// Real implementation that parses `ip -o -4 addr show` output.
pub struct SystemNetworkInterfaces<'a> {
    pub runner: &'a dyn CommandRunner,
}

fn parse_ip_addr_show(output: &str) -> Vec<(String, Ipv4AddrInfo)> {
    let mut result = Vec::new();
    for line in output.lines() {
        // e.g. "2: eth0    inet 192.168.1.10/24 brd 192.168.1.255 scope global eth0"
        let Some(colon) = line.find(':') else { continue };
        let rest = &line[colon + 1..];
        let mut parts = rest.split_whitespace();
        let Some(interface) = parts.next() else { continue };
        let mut info = Ipv4AddrInfo::default();
        let tokens: Vec<&str> = parts.collect();
        for i in 0..tokens.len() {
            match tokens[i] {
                "inet" => {
                    if let Some(cidr) = tokens.get(i + 1) {
                        if let Some((ip, prefix)) = cidr.split_once('/') {
                            info.addr = Some(ip.to_string());
                            if let Ok(prefix_len) = prefix.parse::<u32>() {
                                info.netmask = Some(prefix_len_to_netmask(prefix_len).to_string());
                            }
                        }
                    }
                }
                "brd" => {
                    if let Some(b) = tokens.get(i + 1) {
                        info.broadcast = Some(b.to_string());
                    }
                }
                _ => {}
            }
        }
        if info.addr.is_some() {
            result.push((interface.to_string(), info));
        }
    }
    result
}

fn prefix_len_to_netmask(prefix_len: u32) -> Ipv4Addr {
    let mask = if prefix_len == 0 { 0 } else { u32::MAX << (32 - prefix_len) };
    Ipv4Addr::from(mask)
}

impl NetworkInterfaces for SystemNetworkInterfaces<'_> {
    fn interfaces(&self) -> Vec<String> {
        let output = self.runner.run(&["ip", "-o", "-4", "addr", "show"]).map(|o| o.stdout).unwrap_or_default();
        let mut names: Vec<String> = parse_ip_addr_show(&output).into_iter().map(|(name, _)| name).collect();
        names.dedup();
        names
    }

    fn ipv4_addresses(&self, interface: &str) -> Result<Vec<Ipv4AddrInfo>, String> {
        let output = self.runner.run(&["ip", "-o", "-4", "addr", "show"]).map(|o| o.stdout).unwrap_or_default();
        Ok(parse_ip_addr_show(&output).into_iter().filter(|(name, _)| name == interface).map(|(_, info)| info).collect())
    }
}

/// A directly connected IPv4 network (interface address + prefix length).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ipv4Network {
    pub base: Ipv4Addr,
    pub prefix_len: u32,
}

impl Ipv4Network {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        let mask = if self.prefix_len == 0 { 0 } else { u32::MAX << (32 - self.prefix_len) };
        (u32::from(ip) & mask) == u32::from(self.base)
    }
}

/// Broadcast addresses for all active network interfaces.
pub fn get_broadcast_addresses(nics: &dyn NetworkInterfaces) -> Vec<String> {
    let mut result = Vec::new();
    for interface in nics.interfaces() {
        let Ok(addrs) = nics.ipv4_addresses(&interface) else {
            continue;
        };
        for addr in addrs {
            if let Some(broadcast) = addr.broadcast {
                result.push(broadcast);
            }
        }
    }
    result
}

/// All directly connected IPv4 networks, paired with their interface name.
pub fn get_local_networks(nics: &dyn NetworkInterfaces) -> Vec<(Ipv4Network, String)> {
    let mut result = Vec::new();
    for interface in nics.interfaces() {
        let Ok(addrs) = nics.ipv4_addresses(&interface) else {
            continue;
        };
        for addr in addrs {
            let (Some(ip_str), Some(mask_str)) = (&addr.addr, &addr.netmask) else {
                continue;
            };
            let (Ok(ip), Ok(mask)) = (Ipv4Addr::from_str(ip_str), Ipv4Addr::from_str(mask_str)) else {
                continue;
            };
            let prefix_len = u32::from(mask).count_ones();
            let base = Ipv4Addr::from(u32::from(ip) & u32::from(mask));
            result.push((Ipv4Network { base, prefix_len }, interface.clone()));
        }
    }
    result
}

/// Whether `ip` falls within one of the given directly connected networks.
pub fn is_on_local_network(ip: &str, local_networks: &[(Ipv4Network, String)]) -> bool {
    let Ok(ip) = Ipv4Addr::from_str(ip) else {
        return false;
    };
    local_networks.iter().any(|(network, _)| network.contains(ip))
}

/// Validate an SMB server address/hostname before passing it to `smbclient`.
pub fn is_valid_server_address(server: &str) -> bool {
    let server = server.trim();
    if server.is_empty() || server.starts_with('-') || server.len() > 253 {
        return false;
    }
    if server.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    if std::net::IpAddr::from_str(server).is_ok() {
        return true;
    }

    let server = server.strip_suffix('.').unwrap_or(server);
    let labels: Vec<&str> = server.split('.').collect();
    if labels.is_empty() {
        return false;
    }
    let label_re = Regex::new(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$").unwrap();
    labels.iter().all(|label| label_re.is_match(label))
}

/// Apply SMB authentication arguments to `cmd`, returning a temporary
/// credentials file path if one was created (caller must clean it up).
pub fn apply_auth_args(
    cmd: &mut Vec<String>,
    username: Option<&str>,
    password: Option<&str>,
    credentials_file: Option<&str>,
) -> Result<Option<std::path::PathBuf>, String> {
    if let Some(cred_file) = credentials_file {
        if std::path::Path::new(cred_file).is_file() {
            cmd.push("--authentication-file".to_string());
            cmd.push(cred_file.to_string());
            return Ok(None);
        }
        return Err(format!("Credentials file not found: {cred_file}"));
    }

    if let Some(user) = username {
        cmd.push("-U".to_string());
        cmd.push(user.to_string());
        if let Some(pass) = password {
            let path = write_credentials_file(user, pass).map_err(|e| e.to_string())?;
            cmd.push("--authentication-file".to_string());
            cmd.push(path.to_string_lossy().to_string());
            return Ok(Some(path));
        }
        // Explicitly disable password prompt to keep this non-interactive.
        cmd.push("-N".to_string());
        return Ok(None);
    }

    cmd.push("-N".to_string());
    Ok(None)
}

fn write_credentials_file(username: &str, password: &str) -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hbos-sambaclient-{}-{}.cred",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default()
    ));
    std::fs::write(&path, format!("username={username}\npassword={password}\n"))?;
    Ok(path)
}

fn safe_unlink(path: &Option<std::path::PathBuf>) {
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SmbServerCandidate {
    pub ip: String,
    pub workgroup: String,
    pub broadcast: String,
}

/// Find SMB servers using `nmblookup` for a specific broadcast address.
pub fn find_smb_servers(runner: &dyn CommandRunner, broadcast_address: &str) -> Vec<SmbServerCandidate> {
    let Some(output) = runner.run(&["nmblookup", "-B", broadcast_address, "--", "WORKGROUP"]) else {
        return Vec::new();
    };
    if !output.success() {
        return Vec::new();
    }
    parse_find_smb_servers_output(&output.stdout, broadcast_address)
}

fn parse_find_smb_servers_output(output: &str, broadcast_address: &str) -> Vec<SmbServerCandidate> {
    let line_re = Regex::new(r"(\d+\.\d+\.\d+\.\d+)\s+(\S+)").unwrap();
    let suffix_re = Regex::new(r"<[0-9a-fA-F]+>").unwrap();
    output
        .lines()
        .filter_map(|line| {
            let caps = line_re.captures(line)?;
            let ip = caps.get(1)?.as_str().to_string();
            let name = caps.get(2)?.as_str();
            let workgroup = suffix_re.replace_all(name, "").to_string();
            Some(SmbServerCandidate { ip, workgroup, broadcast: broadcast_address.to_string() })
        })
        .collect()
}

/// Check whether `ip_address` is a file server (`<20>` NetBIOS flag),
/// returning its hostname if so.
pub fn is_file_server(runner: &dyn CommandRunner, ip_address: &str) -> Option<String> {
    let output = runner.run(&["nmblookup", "-A", ip_address])?;
    if !output.success() {
        return None;
    }
    parse_is_file_server_output(&output.stdout)
}

fn parse_is_file_server_output(output: &str) -> Option<String> {
    let hostname = output.lines().find(|l| l.contains("<00>")).and_then(|l| l.split_whitespace().next()).map(|s| s.to_string());
    let is_file_service = output.lines().any(|l| l.contains("<20>") && l.contains("ACTIVE"));
    if is_file_service { hostname } else { None }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HostInfo {
    pub hostname: String,
    pub workgroup: String,
    pub services: Vec<String>,
}

/// Get detailed host information for an IP address using `nmblookup`.
pub fn get_host_info(runner: &dyn CommandRunner, ip_address: &str) -> HostInfo {
    let mut info = HostInfo::default();
    let Some(output) = runner.run(&["nmblookup", "-A", ip_address]) else {
        return info;
    };
    if !output.success() {
        return info;
    }
    parse_host_info_output(&output.stdout, &mut info);
    info
}

fn parse_host_info_output(output: &str, info: &mut HostInfo) {
    let line_re = Regex::new(r"(\S+)\s+<([0-9a-fA-F]+)>\s+(.+)").unwrap();
    for line in output.lines() {
        let Some(caps) = line_re.captures(line) else { continue };
        let name = caps.get(1).unwrap().as_str();
        if line.contains("<00>") && line.contains("UNIQUE") && info.hostname.is_empty() {
            info.hostname = name.to_string();
        } else if line.contains("<00>") && line.contains("GROUP") && info.workgroup.is_empty() {
            info.workgroup = name.to_string();
        } else if line.contains("<20>") && line.contains("ACTIVE") {
            info.services.push("File Server".to_string());
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct DiscoveredServer {
    pub ip: String,
    pub workgroup: String,
    pub broadcast: String,
    pub local_network: Option<String>,
    pub interface: Option<String>,
    pub is_file_server: bool,
    pub hostname: String,
    pub services: Vec<String>,
}

/// List all SMB servers on the network by querying all broadcast addresses.
pub fn list_all_servers(runner: &dyn CommandRunner, nics: &dyn NetworkInterfaces) -> Vec<DiscoveredServer> {
    let broadcasts = get_broadcast_addresses(nics);
    let local_networks = get_local_networks(nics);
    if broadcasts.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for broadcast in &broadcasts {
        candidates.extend(find_smb_servers(runner, broadcast));
    }

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for candidate in candidates {
        if !seen.insert(candidate.ip.clone()) || !is_on_local_network(&candidate.ip, &local_networks) {
            continue;
        }

        let mut server = DiscoveredServer {
            ip: candidate.ip.clone(),
            workgroup: candidate.workgroup.clone(),
            broadcast: candidate.broadcast.clone(),
            ..Default::default()
        };
        if let Ok(ip) = Ipv4Addr::from_str(&candidate.ip) {
            if let Some((network, interface)) = local_networks.iter().find(|(n, _)| n.contains(ip)) {
                server.local_network = Some(format!("{}/{}", network.base, network.prefix_len));
                server.interface = Some(interface.clone());
            }
        }

        if let Some(hostname) = is_file_server(runner, &candidate.ip) {
            server.is_file_server = true;
            server.hostname = hostname;
            server.services = vec!["File Server".to_string()];
        } else {
            let host_info = get_host_info(runner, &candidate.ip);
            server.hostname = host_info.hostname;
            if !host_info.workgroup.is_empty() {
                server.workgroup = host_info.workgroup;
            }
            server.services = host_info.services;
        }

        result.push(server);
    }
    result
}

fn categorize_connection_error(error_output: &str, server: &str, has_username: bool, returncode: i32, stderr: &str, stdout: &str) -> String {
    let lower = error_output.to_lowercase();
    if lower.contains("connection refused") || lower.contains("no route to host") {
        format!("Server {server} is not reachable")
    } else if lower.contains("host not found") || lower.contains("name or service not known") {
        format!("Server {server} not found")
    } else if lower.contains("connection timed out") {
        format!("Connection to {server} timed out")
    } else if lower.contains("authentication failed") || lower.contains("logon failure") || lower.contains("access denied") {
        "Authentication failed".to_string()
    } else if lower.contains("session setup failed") {
        if has_username { "Authentication failed".to_string() } else { "Server requires authentication".to_string() }
    } else if lower.contains("protocol negotiation failed") {
        "SMB protocol negotiation failed".to_string()
    } else if lower.contains("tree connect failed") {
        "Failed to connect to server shares".to_string()
    } else if !stderr.trim().is_empty() {
        format!("Connection failed: {}", stderr.trim())
    } else if !stdout.trim().is_empty() {
        format!("Connection failed: {}", stdout.trim())
    } else {
        format!("Connection failed with error code {returncode}")
    }
}

/// Check if a connection to the specified SMB server is possible with the given credentials.
pub fn check_smb_connection(
    runner: &dyn CommandRunner,
    server: &str,
    username: Option<&str>,
    password: Option<&str>,
    credentials_file: Option<&str>,
) -> Result<(), String> {
    if !runner.which("smbclient") {
        return Err("smbclient command not found".to_string());
    }

    let server = server.trim();
    if !is_valid_server_address(server) {
        return Err("Invalid SMB server address".to_string());
    }

    let mut cmd = vec!["smbclient".to_string(), "-L".to_string(), server.to_string()];
    let cred_path = apply_auth_args(&mut cmd, username, password, credentials_file)?;

    let args: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
    let result = runner.run(&args);
    safe_unlink(&cred_path);

    match result {
        Some(output) if output.success() => Ok(()),
        Some(output) => {
            let combined = format!("{} {}", output.stderr, output.stdout).to_lowercase();
            Err(categorize_connection_error(&combined, server, username.is_some(), output.status, &output.stderr, &output.stdout))
        }
        None => Err("Unknown connection error".to_string()),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct SmbShare {
    pub name: String,
    #[serde(rename = "type")]
    pub share_type: String,
    pub comment: String,
}

fn version_option_args(version: &str) -> Vec<&'static str> {
    match version {
        "SMB1" => vec!["--option=client min protocol=NT1"],
        "SMB2" => vec!["--option=client min protocol=SMB2", "--option=client max protocol=SMB2"],
        "SMB3" => vec!["--option=client min protocol=SMB3"],
        _ => Vec::new(),
    }
}

fn parse_share_table(output: &str) -> Vec<SmbShare> {
    let mut shares = Vec::new();
    let mut in_share_section = false;
    for line in output.lines() {
        if line.contains("Sharename") && line.contains("Type") && line.contains("Comment") {
            in_share_section = true;
            continue;
        }
        if !in_share_section {
            continue;
        }
        let stripped = line.trim();
        if stripped.is_empty() {
            continue;
        }
        let without_spaces: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
        if !without_spaces.is_empty() && without_spaces.chars().all(|c| c == '-') {
            continue;
        }
        if !(line.starts_with(' ') || line.starts_with('\t')) {
            break;
        }
        let parts: Vec<&str> = stripped.split_whitespace().collect();
        let Some(&share_name) = parts.first() else { continue };
        if share_name == "IPC$" || (share_name.ends_with('$') && share_name != "IPC$") {
            continue;
        }
        let share_type = parts.get(1).unwrap_or(&"").to_string();
        let comment = if parts.len() > 2 { parts[2..].join(" ") } else { String::new() };
        shares.push(SmbShare { name: share_name.to_string(), share_type, comment });
    }
    shares
}

/// List available shares on the specified SMB server, trying SMB versions
/// in order (SMB3, SMB2, SMB1) unless one is specified.
pub fn list_smb_shares(
    runner: &dyn CommandRunner,
    server: &str,
    username: Option<&str>,
    password: Option<&str>,
    credentials_file: Option<&str>,
    smb_version: Option<&str>,
) -> (Vec<SmbShare>, String) {
    if !runner.which("smbclient") {
        return (Vec::new(), "Unknown".to_string());
    }

    let versions: Vec<&str> = match smb_version {
        Some(v) => vec![v],
        None => vec!["SMB3", "SMB2", "SMB1"],
    };

    for version in versions {
        let mut cmd = vec!["smbclient".to_string(), "-L".to_string(), server.to_string()];
        for opt in version_option_args(version) {
            cmd.push(opt.to_string());
        }

        let cred_path = match apply_auth_args(&mut cmd, username, password, credentials_file) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let args: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        let result = runner.run(&args);
        safe_unlink(&cred_path);

        if let Some(output) = result {
            if output.success() {
                let shares = parse_share_table(&output.stdout);
                if !shares.is_empty() || smb_version.is_some() {
                    return (shares, version.to_string());
                }
                // Successful connection but no shares: keep first successful version.
                return (shares, version.to_string());
            }
        }
    }

    (Vec::new(), "Unknown".to_string())
}

/// Detect the SMB version supported by the specified server.
pub fn detect_smb_version(
    runner: &dyn CommandRunner,
    server: &str,
    username: Option<&str>,
    password: Option<&str>,
    credentials_file: Option<&str>,
) -> String {
    if !runner.which("smbclient") {
        return "Unknown".to_string();
    }

    for version in ["SMB3", "SMB2", "SMB1"] {
        let mut cmd = vec!["smbclient".to_string(), "-L".to_string(), server.to_string()];
        for opt in version_option_args(version) {
            cmd.push(opt.to_string());
        }

        let cred_path = match apply_auth_args(&mut cmd, username, password, credentials_file) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let args: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        let result = runner.run(&args);
        safe_unlink(&cred_path);

        if let Some(output) = result {
            if output.success() {
                return version.to_string();
            }
        }
    }

    "Unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeNics {
        interfaces: Vec<String>,
        addrs: std::collections::HashMap<String, Result<Vec<Ipv4AddrInfo>, String>>,
    }

    impl NetworkInterfaces for FakeNics {
        fn interfaces(&self) -> Vec<String> {
            self.interfaces.clone()
        }
        fn ipv4_addresses(&self, interface: &str) -> Result<Vec<Ipv4AddrInfo>, String> {
            self.addrs.get(interface).cloned().unwrap_or(Ok(Vec::new()))
        }
    }

    #[test]
    fn get_broadcast_addresses_empty_when_no_interfaces() {
        let nics = FakeNics { interfaces: Vec::new(), addrs: Default::default() };
        assert_eq!(get_broadcast_addresses(&nics), Vec::<String>::new());
    }

    #[test]
    fn get_broadcast_addresses_skips_broken_interface() {
        let mut addrs = std::collections::HashMap::new();
        addrs.insert("eth0".to_string(), Err("device error".to_string()));
        addrs.insert(
            "wlan0".to_string(),
            Ok(vec![Ipv4AddrInfo { addr: Some("192.168.1.10".to_string()), netmask: None, broadcast: Some("192.168.1.255".to_string()) }]),
        );
        let nics = FakeNics { interfaces: vec!["eth0".to_string(), "wlan0".to_string()], addrs };
        assert_eq!(get_broadcast_addresses(&nics), vec!["192.168.1.255".to_string()]);
    }

    #[test]
    fn get_local_networks_skips_broken_interface() {
        let mut addrs = std::collections::HashMap::new();
        addrs.insert("eth0".to_string(), Err("bad interface".to_string()));
        addrs.insert(
            "wlan0".to_string(),
            Ok(vec![Ipv4AddrInfo { addr: Some("10.0.0.20".to_string()), netmask: Some("255.255.255.0".to_string()), broadcast: None }]),
        );
        let nics = FakeNics { interfaces: vec!["eth0".to_string(), "wlan0".to_string()], addrs };

        let networks = get_local_networks(&nics);
        assert_eq!(networks.len(), 1);
        let (network, interface) = &networks[0];
        assert_eq!(interface, "wlan0");
        assert_eq!(network.base, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(network.prefix_len, 24);
    }

    #[test]
    fn is_on_local_network_matches_containing_network() {
        let networks = vec![(Ipv4Network { base: Ipv4Addr::new(10, 0, 0, 0), prefix_len: 24 }, "wlan0".to_string())];
        assert!(is_on_local_network("10.0.0.55", &networks));
        assert!(!is_on_local_network("192.168.1.5", &networks));
        assert!(!is_on_local_network("not-an-ip", &networks));
    }

    #[test]
    fn is_valid_server_address_accepts_hostnames_and_ips() {
        assert!(is_valid_server_address("server"));
        assert!(is_valid_server_address("my-server.local"));
        assert!(is_valid_server_address("192.168.1.1"));
        assert!(is_valid_server_address("::1"));
    }

    #[test]
    fn is_valid_server_address_rejects_option_like_and_invalid_values() {
        assert!(!is_valid_server_address("--help"));
        assert!(!is_valid_server_address(""));
        assert!(!is_valid_server_address("has space"));
        assert!(!is_valid_server_address(&"a".repeat(254)));
    }

    struct StubRunner {
        which: bool,
        calls: Mutex<Vec<Vec<String>>>,
        response: Option<CommandOutput>,
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
    fn check_connection_username_without_password_is_non_interactive() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput { status: 0, stdout: "ok".to_string(), stderr: String::new() }),
        };

        let result = check_smb_connection(&runner, "server", Some("alice"), None, None);
        assert!(result.is_ok());

        let calls = runner.calls.lock().unwrap();
        let cmd = &calls[0];
        assert!(cmd.contains(&"-U".to_string()));
        assert!(cmd.contains(&"alice".to_string()));
        assert!(cmd.contains(&"-N".to_string()));
    }

    #[test]
    fn check_connection_missing_credentials_file_returns_error() {
        let runner = StubRunner { which: true, calls: Mutex::new(Vec::new()), response: None };
        let result = check_smb_connection(&runner, "server", None, None, Some("/no/such/file"));
        assert!(result.unwrap_err().contains("Credentials file not found"));
    }

    #[test]
    fn check_connection_rejects_invalid_server_address() {
        let runner = StubRunner { which: true, calls: Mutex::new(Vec::new()), response: None };
        let result = check_smb_connection(&runner, "--help", None, None, None);
        assert_eq!(result, Err("Invalid SMB server address".to_string()));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn list_shares_username_without_password_uses_non_interactive_auth() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput {
                status: 0,
                stdout: "Sharename       Type      Comment\n---------       ----      -------\n  public        Disk      Public Share\n".to_string(),
                stderr: String::new(),
            }),
        };

        let (shares, detected) = list_smb_shares(&runner, "server", Some("alice"), None, None, None);
        assert_eq!(detected, "SMB3");
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].name, "public");

        let calls = runner.calls.lock().unwrap();
        let cmd = &calls[0];
        assert!(cmd.contains(&"-U".to_string()));
        assert!(cmd.contains(&"alice".to_string()));
        assert!(cmd.contains(&"-N".to_string()));
    }

    #[test]
    fn detect_version_username_without_password_uses_non_interactive_auth() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() }),
        };

        let version = detect_smb_version(&runner, "server", Some("alice"), None, None);
        assert_eq!(version, "SMB3");

        let calls = runner.calls.lock().unwrap();
        let cmd = &calls[0];
        assert!(cmd.contains(&"-U".to_string()));
        assert!(cmd.contains(&"alice".to_string()));
        assert!(cmd.contains(&"-N".to_string()));
    }

    #[test]
    fn list_shares_missing_credentials_file_returns_unknown() {
        let runner = StubRunner { which: true, calls: Mutex::new(Vec::new()), response: None };
        let (shares, detected) = list_smb_shares(&runner, "server", None, None, Some("/missing"), None);
        assert_eq!(shares, Vec::<SmbShare>::new());
        assert_eq!(detected, "Unknown");
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn get_host_info_services_is_list() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput {
                status: 0,
                stdout: "HOSTNAME        <00> -         UNIQUE\nWORKGROUP       <00> -         GROUP\nHOSTNAME        <20> -         ACTIVE\n".to_string(),
                stderr: String::new(),
            }),
        };

        let info = get_host_info(&runner, "192.168.1.10");
        assert_eq!(info.hostname, "HOSTNAME");
        assert_eq!(info.workgroup, "WORKGROUP");
        assert_eq!(info.services, vec!["File Server".to_string()]);
    }

    #[test]
    fn is_file_server_detects_active_20_flag() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput { status: 0, stdout: "HOSTNAME        <00> -         UNIQUE\nHOSTNAME        <20> -         ACTIVE\n".to_string(), stderr: String::new() }),
        };
        assert_eq!(is_file_server(&runner, "192.168.1.10"), Some("HOSTNAME".to_string()));
    }

    #[test]
    fn is_file_server_returns_none_without_active_20_flag() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: Some(CommandOutput { status: 0, stdout: "HOSTNAME        <00> -         UNIQUE\n".to_string(), stderr: String::new() }),
        };
        assert_eq!(is_file_server(&runner, "192.168.1.10"), None);
    }
}
