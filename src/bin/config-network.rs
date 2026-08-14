//! Standalone `config-network` command-line tool.
use clap::Parser;
use hbos_configurator::cli::network_cli::{run, NetworkArgs};

fn main() {
    let args = NetworkArgs::parse();
    std::process::exit(run(&args));
}
