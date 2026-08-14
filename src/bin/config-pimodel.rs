//! Standalone `config-pimodel` command-line tool.
use clap::Parser;
use hbos_configurator::cli::pimodel_cli::{run, PimodelArgs};

fn main() {
    let args = PimodelArgs::parse();
    run(&args);
}
