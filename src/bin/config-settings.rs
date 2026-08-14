//! Standalone `config-settings` command-line tool.
use clap::Parser;
use hbos_configurator::cli::settings_manager_cli::{run, SettingsArgs};

fn main() {
    let args = SettingsArgs::parse();
    std::process::exit(run(&args));
}
