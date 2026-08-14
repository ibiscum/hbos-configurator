//! Standalone `config-systeminfo` command-line tool.
use clap::Parser;
use hbos_configurator::cli::systeminfo_cli::{run, SystemInfoArgs};

fn main() {
    let args = SystemInfoArgs::parse();
    run(&args);
}
