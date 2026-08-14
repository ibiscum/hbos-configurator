//! CLI wrapper around `configurator/settings_manager.py` functionality (the
//! Python module has no standalone `main()`; this exposes the same
//! operations as the `config-settings` tool for scripting/debugging).
use clap::{Parser, Subcommand};

use crate::api::settings_manager::{MemoryConfigDb, SettingsManager};

#[derive(Parser, Debug, PartialEq)]
#[command(name = "config-settings", about = "Manage saved HiFiBerry configuration settings")]
pub struct SettingsArgs {
    #[command(subcommand)]
    pub command: SettingsCommand,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum SettingsCommand {
    /// List registered and saved setting names
    List,
    /// Save all registered settings
    SaveAll,
    /// Restore all registered settings
    RestoreAll,
    /// Delete a saved setting
    Delete {
        /// Name of the setting to delete
        name: String,
    },
}

/// Run the settings CLI. Returns the process exit code.
pub fn run(args: &SettingsArgs) -> i32 {
    // No modules register save/restore callbacks yet, so each invocation
    // operates on an empty in-memory config db; this will grow as other
    // modules are wired into the shared settings manager.
    let mut mgr = SettingsManager::new(Box::new(MemoryConfigDb::default()));

    match &args.command {
        SettingsCommand::List => {
            println!("Registered settings: {:?}", mgr.list_registered_settings());
            println!("Saved settings: {:?}", mgr.list_saved_settings());
            0
        }
        SettingsCommand::SaveAll => {
            let results = mgr.save_all_settings();
            println!("{results:?}");
            0
        }
        SettingsCommand::RestoreAll => {
            let results = mgr.restore_all_settings();
            println!("{results:?}");
            0
        }
        SettingsCommand::Delete { name } => {
            if mgr.delete_saved_setting(name) {
                println!("Deleted saved setting '{name}'");
                0
            } else {
                eprintln!("Setting name must not be empty");
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_command() {
        let args = SettingsArgs::parse_from(["config-settings", "list"]);
        assert_eq!(args.command, SettingsCommand::List);
    }

    #[test]
    fn parses_delete_command_with_name() {
        let args = SettingsArgs::parse_from(["config-settings", "delete", "alpha"]);
        assert_eq!(args.command, SettingsCommand::Delete { name: "alpha".to_string() });
    }

    #[test]
    fn run_list_returns_zero() {
        let args = SettingsArgs::parse_from(["config-settings", "list"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_delete_rejects_empty_name() {
        let args = SettingsCommand::Delete { name: String::new() };
        let wrapped = SettingsArgs { command: args };
        assert_eq!(run(&wrapped), 1);
    }

    #[test]
    fn run_delete_accepts_name() {
        let args = SettingsArgs::parse_from(["config-settings", "delete", "alpha"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_save_all_and_restore_all_return_zero() {
        let save_args = SettingsArgs::parse_from(["config-settings", "save-all"]);
        assert_eq!(run(&save_args), 0);
        let restore_args = SettingsArgs::parse_from(["config-settings", "restore-all"]);
        assert_eq!(run(&restore_args), 0);
    }
}
