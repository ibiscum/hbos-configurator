//! Standalone `config-configdb` command-line tool.
use clap::Parser;
use hbos_configurator::cli::configdb_cli::{run, ConfigdbArgs};

fn main() {
    let args = ConfigdbArgs::parse();
    std::process::exit(run(&args));
}
