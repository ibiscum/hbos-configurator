//! Standalone `config-sambamount` command-line tool.
use clap::Parser;
use hbos_configurator::cli::sambamount_cli::{run, SambamountArgs};

fn main() {
    let args = SambamountArgs::parse();
    std::process::exit(run(&args));
}
