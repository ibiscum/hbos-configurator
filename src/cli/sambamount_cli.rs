//! CLI wrapper mirroring `configurator.sambamount:main` (the `config-sambamount` tool).
use std::fs;

use clap::{ArgGroup, Parser};

use crate::api::sambamount::{self, MemoryConfigDb, SystemCommandRunner};

#[derive(Parser, Debug, PartialEq)]
#[command(
    name = "config-sambamount",
    about = "SMB Mount Management Tool",
    group(ArgGroup::new("command").required(true).args([
        "add_mount", "remove_mount", "mount_all", "mount", "unmount", "list_mounts", "list_mounted_dirs",
    ]))
)]
pub struct SambamountArgs {
    /// Add a mount configuration to the config database
    #[arg(long = "add-mount")]
    pub add_mount: bool,

    /// Remove a mount configuration from the config database
    #[arg(long = "remove-mount")]
    pub remove_mount: bool,

    /// Mount all shares defined in the config database
    #[arg(long = "mount-all")]
    pub mount_all: bool,

    /// Mount a specific share (requires --server and --share)
    #[arg(long)]
    pub mount: bool,

    /// Unmount a specific share (requires --server and --share)
    #[arg(long)]
    pub unmount: bool,

    /// List all configured mounts
    #[arg(long = "list-mounts")]
    pub list_mounts: bool,

    /// List only directories that are currently mounted (one per line)
    #[arg(long = "list-mounted-dirs")]
    pub list_mounted_dirs: bool,

    /// Server name or IP address (for mount operations)
    #[arg(long)]
    pub server: Option<String>,

    /// Share name (for mount operations)
    #[arg(long)]
    pub share: Option<String>,

    /// Username for connection
    #[arg(long)]
    pub user: Option<String>,

    /// Password for connection
    #[arg(long)]
    pub password: Option<String>,

    /// Mount point (default: /data/server-share)
    #[arg(long)]
    pub mountpoint: Option<String>,

    /// SMB protocol version to use
    #[arg(long, value_parser = ["SMB1", "SMB2", "SMB3"])]
    pub version: Option<String>,

    /// Additional mount options for CIFS mounts
    #[arg(long = "mount-options", default_value = "")]
    pub mount_options: String,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress all output except warnings and errors
    #[arg(short, long)]
    pub quiet: bool,
}

fn read_proc_mounts() -> String {
    fs::read_to_string("/proc/mounts").unwrap_or_default()
}

/// Run the sambamount CLI. Returns the process exit code (mirrors Python's `sys.exit`).
pub fn run(args: &SambamountArgs) -> i32 {
    // Persistence is not wired to a shared/global config db yet (no
    // persistent ConfigDB implementation has been ported), so each
    // invocation starts from an empty in-memory store.
    let mut db = MemoryConfigDb::default();
    let runner = SystemCommandRunner;
    let proc_mounts = read_proc_mounts();

    if args.add_mount {
        let (Some(server), Some(share)) = (&args.server, &args.share) else {
            eprintln!("--add-mount requires --server and --share");
            return 1;
        };
        match sambamount::add_mount_config(
            &mut db,
            server,
            share,
            args.mountpoint.as_deref(),
            args.user.as_deref(),
            args.password.as_deref(),
            args.version.as_deref(),
            Some(&args.mount_options),
        ) {
            Ok(()) => {
                println!("Successfully added mount configuration for {server}/{share}");
                0
            }
            Err(e) => {
                eprintln!("Failed to add mount configuration for {server}/{share}: {e}");
                1
            }
        }
    } else if args.remove_mount {
        let (Some(server), Some(share)) = (&args.server, &args.share) else {
            eprintln!("--remove-mount requires --server and --share");
            return 1;
        };
        match sambamount::remove_mount_config(&mut db, server, share) {
            Ok(mountpoint) => {
                println!("Successfully removed mount configuration for {server}/{share}");
                if !mountpoint.is_empty() {
                    println!("Share was unmounted from {mountpoint}");
                }
                0
            }
            Err(e) => {
                eprintln!("Failed to remove mount configuration for {server}/{share}: {e}");
                1
            }
        }
    } else if args.mount_all {
        let mounts = sambamount::read_mount_config(&db);
        let results = sambamount::mount_all_shares(&runner, &proc_mounts, &mounts);
        if !results.succeeded.is_empty() {
            println!("Successfully mounted {} shares:", results.succeeded.len());
            for m in &results.succeeded {
                println!("  - {m}");
            }
        }
        if !results.failed.is_empty() {
            eprintln!("Failed to mount {} shares:", results.failed.len());
            for m in &results.failed {
                eprintln!("  - {m}");
            }
            return 1;
        }
        0
    } else if args.mount {
        let (Some(server), Some(share)) = (&args.server, &args.share) else {
            eprintln!("--mount requires both --server and --share");
            return 1;
        };
        let mounts = sambamount::read_mount_config(&db);
        match sambamount::mount_smb_share(&runner, &proc_mounts, &mounts, server, share) {
            Ok(()) => {
                println!("Successfully mounted {server}/{share}");
                0
            }
            Err(e) => {
                eprintln!("Failed to mount {server}/{share}: {e}");
                1
            }
        }
    } else if args.unmount {
        let (Some(server), Some(share)) = (&args.server, &args.share) else {
            eprintln!("--unmount requires both --server and --share");
            return 1;
        };
        let mounts = sambamount::read_mount_config(&db);
        match sambamount::unmount_smb_share(&runner, &proc_mounts, &mounts, server, share) {
            Ok(()) => {
                println!("Successfully unmounted {server}/{share}");
                0
            }
            Err(e) => {
                eprintln!("Failed to unmount {server}/{share}: {e}");
                1
            }
        }
    } else if args.list_mounts {
        let statuses = sambamount::list_configured_mounts(&db, &proc_mounts);
        if statuses.is_empty() {
            eprintln!("No mount configurations found in configdb");
        } else {
            for status in &statuses {
                let status_icon = if status.mounted { "\u{2713}" } else { "\u{2717}" };
                let status_text = if status.mounted { "MOUNTED" } else { "UNMOUNTED" };
                let user_prefix = if status.config.user.is_empty() { String::new() } else { format!("{}@", status.config.user) };
                let version = if status.config.version.is_empty() { "Auto" } else { &status.config.version };
                println!(
                    "[{}] [{} {}] //{}{}/{} -> {} ({})",
                    status.config.id, status_icon, status_text, user_prefix, status.config.server, status.config.share, status.config.mountpoint, version
                );
            }
        }
        0
    } else {
        // --list-mounted-dirs
        let statuses = sambamount::list_configured_mounts(&db, &proc_mounts);
        for status in &statuses {
            if status.mounted {
                println!("{}", status.config.mountpoint);
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_mounts_command() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--list-mounts"]);
        assert!(args.list_mounts);
    }

    #[test]
    fn parses_add_mount_with_options() {
        let args = SambamountArgs::parse_from([
            "config-sambamount",
            "--add-mount",
            "--server",
            "srv",
            "--share",
            "music",
            "--version",
            "SMB3",
        ]);
        assert!(args.add_mount);
        assert_eq!(args.server, Some("srv".to_string()));
        assert_eq!(args.version, Some("SMB3".to_string()));
    }

    #[test]
    fn run_add_mount_requires_server_and_share() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--add-mount"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_add_mount_succeeds_with_server_and_share() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--add-mount", "--server", "srv", "--share", "music"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_list_mounts_returns_zero_when_empty() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--list-mounts"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_remove_mount_fails_without_existing_config() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--remove-mount", "--server", "srv", "--share", "music"]);
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_mount_fails_without_existing_config() {
        let args = SambamountArgs::parse_from(["config-sambamount", "--mount", "--server", "srv", "--share", "music"]);
        assert_eq!(run(&args), 1);
    }
}
