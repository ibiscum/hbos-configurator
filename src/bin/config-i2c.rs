//! Standalone `config-i2c` command-line tool.
use clap::Parser;
use hbos_configurator::cli::i2c_cli::{run, I2cArgs};

fn main() {
    let args = I2cArgs::parse();
    std::process::exit(run(&args));
}
