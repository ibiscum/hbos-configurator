//! Standalone `config-systemd-service` command-line tool.
use clap::Parser;
use hbos_configurator::cli::systemd_service_cli::{run, SystemdServiceArgs};

fn main() {
    let args = SystemdServiceArgs::parse();
    std::process::exit(run(&args));
}
