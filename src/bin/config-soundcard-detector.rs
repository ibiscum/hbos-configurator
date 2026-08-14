//! Standalone `config-soundcard-detector` command-line tool.
use clap::Parser;
use hbos_configurator::cli::soundcard_detector_cli::{run, SoundcardDetectorArgs};

fn main() {
    let args = SoundcardDetectorArgs::parse();
    run(&args);
}
