//! CLI wrapper mirroring `configurator.network:main` (the `config-network` tool).
use std::path::Path;

use clap::{ArgGroup, Parser};

use crate::api::network::{self, SystemCommandRunner, SystemNetworkInterfaces};

#[derive(Parser, Debug, PartialEq)]
#[command(
    name = "config-network",
    about = "Network Configuration Tool",
    group(ArgGroup::new("command").required(true).args([
        "list_interfaces", "set_dhcp", "set_fixed", "enable_ipv6", "disable_ipv6",
    ]))
)]
pub struct NetworkArgs {
    /// List all physical network interfaces
    #[arg(long = "list-interfaces")]
    pub list_interfaces: bool,

    /// Configure specified interface to use DHCP
    #[arg(long = "set-dhcp", value_name = "INTERFACE")]
    pub set_dhcp: Option<String>,

    /// Configure specified interface to use static IP
    #[arg(long = "set-fixed", value_name = "INTERFACE")]
    pub set_fixed: Option<String>,

    /// Enable IPv6 system-wide on all interfaces (persistent across reboots)
    #[arg(long = "enable-ipv6")]
    pub enable_ipv6: bool,

    /// Disable IPv6 system-wide on all interfaces (persistent across reboots)
    #[arg(long = "disable-ipv6")]
    pub disable_ipv6: bool,

    /// Fixed IP address with netmask (e.g., 192.168.1.10/24)
    #[arg(long)]
    pub ip: Option<String>,

    /// Router/gateway address (e.g., 192.168.1.1)
    #[arg(long)]
    pub router: Option<String>,

    /// Display detailed interface information in a single line
    #[arg(long)]
    pub long: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress all output except warnings and errors
    #[arg(short, long)]
    pub quiet: bool,
}

fn is_physical(runner: &SystemCommandRunner, interface: &str) -> bool {
    network::is_physical_interface(interface, runner, Path::new("/"))
}

/// Run the network CLI. Returns the process exit code (mirrors Python's `sys.exit`).
pub fn run(args: &NetworkArgs) -> i32 {
    let runner = SystemCommandRunner;

    if args.list_interfaces {
        let nics = SystemNetworkInterfaces { runner: &runner };
        let interfaces = network::list_physical_interfaces(&nics, &|iface| is_physical(&runner, iface), Path::new("/"));
        if interfaces.is_empty() {
            eprintln!("No physical network interfaces found");
        } else {
            for interface in &interfaces {
                if args.long {
                    let mac = interface.mac.clone().unwrap_or_else(|| "Unknown".to_string());
                    let ipv4 = interface.ipv4.clone().unwrap_or_else(|| "Not configured".to_string());
                    println!("{} | {} | {} | {} | {}", interface.name, interface.interface_type, mac, ipv4, interface.state);
                } else {
                    println!("{}", interface.name);
                }
            }
        }
        0
    } else if let Some(interface) = &args.set_dhcp {
        if network::configure_dhcp(&runner, interface, &|iface| is_physical(&runner, iface)) {
            println!("Interface {interface} configured to use DHCP");
            0
        } else {
            eprintln!("Failed to configure DHCP on interface {interface}");
            1
        }
    } else if let Some(interface) = &args.set_fixed {
        let (Some(ip), Some(router)) = (&args.ip, &args.router) else {
            eprintln!("--set-fixed requires --ip and --router arguments");
            return 1;
        };
        if network::configure_fixed_ip(&runner, interface, ip, router, &|iface| is_physical(&runner, iface)) {
            println!("Interface {interface} configured with static IP {ip} and router {router}");
            0
        } else {
            eprintln!("Failed to configure static IP on interface {interface}");
            1
        }
    } else if args.enable_ipv6 {
        if network::enable_ipv6(&runner, Path::new("/etc/sysctl.d")) {
            println!("IPv6 enabled system-wide");
            0
        } else {
            eprintln!("Failed to enable IPv6 system-wide");
            1
        }
    } else if args.disable_ipv6 {
        if network::disable_ipv6(&runner, Path::new("/etc/sysctl.d")) {
            println!("IPv6 disabled system-wide");
            0
        } else {
            eprintln!("Failed to disable IPv6 system-wide");
            1
        }
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_interfaces() {
        let args = NetworkArgs::parse_from(["config-network", "--list-interfaces"]);
        assert!(args.list_interfaces);
    }

    #[test]
    fn parses_set_fixed_with_ip_and_router() {
        let args = NetworkArgs::parse_from(["config-network", "--set-fixed", "eth0", "--ip", "192.168.1.10/24", "--router", "192.168.1.1"]);
        assert_eq!(args.set_fixed, Some("eth0".to_string()));
        assert_eq!(args.ip, Some("192.168.1.10/24".to_string()));
        assert_eq!(args.router, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn run_set_fixed_requires_ip_and_router() {
        let args = NetworkArgs::parse_from(["config-network", "--set-fixed", "eth0"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_set_dhcp_fails_without_network_manager() {
        let args = NetworkArgs::parse_from(["config-network", "--set-dhcp", "eth0"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_list_interfaces_returns_zero() {
        let args = NetworkArgs::parse_from(["config-network", "--list-interfaces"]);
        assert_eq!(run(&args), 0);
    }
}
