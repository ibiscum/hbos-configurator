//! Standalone `config-pipewire` command-line tool.
use clap::Parser;
use hbos_configurator::cli::pipewire_cli::{run, PipewireArgs};

fn main() {
    let args = PipewireArgs::parse();
    std::process::exit(run(&args));
}
