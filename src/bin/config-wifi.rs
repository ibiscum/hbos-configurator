//! Standalone `config-wifi` command-line tool.
use clap::Parser;
use hbos_configurator::cli::wifi_cli::{run, WifiArgs};

fn main() {
    let args = WifiArgs::parse();
    std::process::exit(run(&args));
}
