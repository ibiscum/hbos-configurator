//! Standalone `config-soundcard` command-line tool.
use clap::Parser;
use hbos_configurator::cli::soundcard_cli::{run, SoundcardArgs};

fn main() {
    let args = SoundcardArgs::parse();
    std::process::exit(run(&args));
}
