//! Standalone `config-dsptoolkit` command-line tool.
use clap::Parser;
use hbos_configurator::cli::dsptoolkit_cli::{run, DsptoolkitArgs};

fn main() {
    let args = DsptoolkitArgs::parse();
    std::process::exit(run(&args));
}
