//! CLI wrapper mirroring `configurator.soundcard_detector:main` (the
//! `config-soundcard-detector` tool).
use std::fs;
use std::path::PathBuf;

use clap::Parser;

use crate::api::soundcard_detector::SoundcardDetector;
use crate::api::systeminfo::read_hat_info;

const CONFIG_TXT_PATH: &str = "/boot/firmware/config.txt";
const REBOOT_FILE_PATH: &str = "/tmp/reboot";

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-soundcard-detector", about = "HiFiBerry Sound Card Detector")]
pub struct SoundcardDetectorArgs {
    /// Store detected card configuration in config.txt
    #[arg(long)]
    pub store: bool,

    /// Assume DAC+ Light if no card is detected
    #[arg(long)]
    pub fallback_dac: bool,

    /// Enable verbose output showing each detection step
    #[arg(short, long)]
    pub verbose: bool,
}

fn read_config_lines(path: &str) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| content.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

/// Run the soundcard-detector CLI: detects the connected HiFiBerry card and
/// prints its name (or persists it to config.txt with `--store`).
pub fn run(args: &SoundcardDetectorArgs) {
    if args.verbose {
        tracing::debug!("verbose logging enabled for config-soundcard-detector");
    }

    let config_lines = read_config_lines(CONFIG_TXT_PATH);
    let hat = read_hat_info(std::path::Path::new("/"));

    let mut detector = SoundcardDetector::new(config_lines, PathBuf::from(REBOOT_FILE_PATH));
    let card = detector
        .detect_and_configure(hat.product.as_deref(), args.store, args.fallback_dac)
        .unwrap_or(None);

    if args.store {
        if let Err(e) = fs::write(CONFIG_TXT_PATH, detector.config_lines.join("\n") + "\n") {
            tracing::error!("Failed to write {}: {}", CONFIG_TXT_PATH, e);
        }
    }

    println!("{}", card.unwrap_or_else(|| "Unknown".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = SoundcardDetectorArgs::parse_from(["config-soundcard-detector"]);
        assert!(!args.store);
        assert!(!args.fallback_dac);
        assert!(!args.verbose);
    }

    #[test]
    fn parses_store_and_fallback_flags() {
        let args = SoundcardDetectorArgs::parse_from([
            "config-soundcard-detector",
            "--store",
            "--fallback-dac",
            "-v",
        ]);
        assert!(args.store);
        assert!(args.fallback_dac);
        assert!(args.verbose);
    }
}
