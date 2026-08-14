//! Standalone `config-sambaclient` command-line tool.
use clap::Parser;
use hbos_configurator::cli::sambaclient_cli::{run, SambaclientArgs};

fn main() {
    let args = SambaclientArgs::parse();
    std::process::exit(run(&args));
}
