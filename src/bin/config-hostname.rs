//! Standalone `config-hostname` command-line tool.
use clap::Parser;
use hbos_configurator::cli::hostname_cli::{run, HostnameArgs};

fn main() {
    let args = HostnameArgs::parse();
    std::process::exit(run(&args));
}
