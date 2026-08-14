//! Standalone `config-hattools` command-line tool.
use clap::Parser;
use hbos_configurator::cli::hattools_cli::{run, HattoolsArgs};

fn main() {
    let args = HattoolsArgs::parse();
    std::process::exit(run(&args));
}
