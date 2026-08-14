//! CLI wrapper mirroring `configurator.sambaclient:main` (the `config-sambaclient` tool).
use clap::{ArgGroup, Parser};

use crate::api::sambaclient::{self, SystemCommandRunner, SystemNetworkInterfaces};

#[derive(Parser, Debug, PartialEq)]
#[command(
    name = "config-sambaclient",
    about = "SMB/CIFS client tools",
    group(ArgGroup::new("command").required(true).args([
        "list_file_servers", "check_connect", "detect_version", "list_shares",
    ]))
)]
pub struct SambaclientArgs {
    /// Username for authentication
    #[arg(short = 'u', long)]
    pub user: Option<String>,

    /// Password for authentication
    #[arg(short = 'p', long)]
    pub password: Option<String>,

    /// Path to credentials file
    #[arg(short = 'c', long)]
    pub credentials: Option<String>,

    /// Specify SMB version to use
    #[arg(long, value_parser = ["SMB1", "SMB2", "SMB3"])]
    pub smbversion: Option<String>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// List all SMB file servers on the network
    #[arg(long = "list-file-servers")]
    pub list_file_servers: bool,

    /// Test connection to specified server
    #[arg(long = "check-connect", value_name = "SERVER")]
    pub check_connect: Option<String>,

    /// Detect SMB version of specified server
    #[arg(long = "detect-version", value_name = "SERVER")]
    pub detect_version: Option<String>,

    /// List shares on specified server
    #[arg(long = "list-shares", value_name = "SERVER")]
    pub list_shares: Option<String>,

    /// Use detailed format when listing shares
    #[arg(long)]
    pub long: bool,
}

/// Run the sambaclient CLI. Returns the process exit code (mirrors Python's `sys.exit`).
pub fn run(args: &SambaclientArgs) -> i32 {
    if args.password.is_some() && args.user.is_none() {
        eprintln!("Password provided without username (--password requires --user)");
        return 1;
    }

    let runner = SystemCommandRunner;

    if args.list_file_servers {
        let nics = SystemNetworkInterfaces { runner: &runner };
        let servers = sambaclient::list_all_servers(&runner, &nics);
        for server in servers.iter().filter(|s| s.is_file_server) {
            let hostname = if !server.hostname.is_empty() { server.hostname.clone() } else { server.workgroup.clone() };
            println!("{}\t{}", server.ip, hostname);
        }
        return 0;
    }

    if let Some(server) = &args.check_connect {
        match sambaclient::check_smb_connection(&runner, server, args.user.as_deref(), args.password.as_deref(), args.credentials.as_deref()) {
            Ok(()) => {
                println!("Connection successful");
                0
            }
            Err(e) => {
                println!("Connection failed: {e}");
                1
            }
        }
    } else if let Some(server) = &args.detect_version {
        let version = sambaclient::detect_smb_version(&runner, server, args.user.as_deref(), args.password.as_deref(), args.credentials.as_deref());
        println!("SMB Version: {version}");
        if version != "Unknown" {
            0
        } else {
            eprintln!("Could not detect SMB version for {server}");
            1
        }
    } else if let Some(server) = &args.list_shares {
        let (shares, _detected) =
            sambaclient::list_smb_shares(&runner, server, args.user.as_deref(), args.password.as_deref(), args.credentials.as_deref(), args.smbversion.as_deref());
        if shares.is_empty() {
            eprintln!("No accessible shares found on {server}");
            return 1;
        }
        for share in &shares {
            if args.long {
                println!("{};{}", share.name, share.comment);
            } else {
                println!("{}", share.name);
            }
        }
        0
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_check_connect_with_credentials() {
        let args = SambaclientArgs::parse_from(["config-sambaclient", "--check-connect", "server", "-u", "alice", "-p", "secret"]);
        assert_eq!(args.check_connect, Some("server".to_string()));
        assert_eq!(args.user, Some("alice".to_string()));
        assert_eq!(args.password, Some("secret".to_string()));
    }

    #[test]
    fn run_rejects_password_without_username() {
        let args = SambaclientArgs::parse_from(["config-sambaclient", "--check-connect", "server", "--password", "secret"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_check_connect_fails_for_unreachable_server() {
        let args = SambaclientArgs::parse_from(["config-sambaclient", "--check-connect", "unreachable.invalid"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_list_shares_fails_without_reachable_server() {
        let args = SambaclientArgs::parse_from(["config-sambaclient", "--list-shares", "unreachable.invalid"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_list_file_servers_returns_zero() {
        let args = SambaclientArgs::parse_from(["config-sambaclient", "--list-file-servers"]);
        assert_eq!(run(&args), 0);
    }
}
