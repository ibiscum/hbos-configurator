//! CLI wrapper mirroring `configurator.hattools:main` (the `config-hattools` tool).
use std::path::Path;

use clap::Parser;

use crate::api::hattools::{get_hat_info, SysfsHatEepromReader, DEFAULT_PRODUCT, DEFAULT_UUID, DEFAULT_VENDOR};

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-hattools", about = "Retrieve HAT information")]
pub struct HattoolsArgs {
    /// Display vendor, product, and UUID
    #[arg(short, long)]
    pub all: bool,

    /// Enable verbose error messages
    #[arg(short, long)]
    pub verbose: bool,
}

/// Run the hattools CLI. Always returns 0 (mirrors Python's `main()`).
pub fn run(args: &HattoolsArgs) -> i32 {
    let reader = SysfsHatEepromReader { root: Path::new("/") };
    let info = get_hat_info(&reader, args.verbose);

    let vendor = info.vendor.unwrap_or_else(|| DEFAULT_VENDOR.to_string());
    let product = info.product.unwrap_or_else(|| DEFAULT_PRODUCT.to_string());
    let uuid = info.uuid.unwrap_or_else(|| DEFAULT_UUID.to_string());

    if args.all {
        println!("{vendor}:{product}:{uuid}");
    } else {
        println!("{vendor}:{product}");
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = HattoolsArgs::parse_from(["config-hattools"]);
        assert!(!args.all);
        assert!(!args.verbose);
    }

    #[test]
    fn parses_all_and_verbose_flags() {
        let args = HattoolsArgs::parse_from(["config-hattools", "--all", "--verbose"]);
        assert!(args.all);
        assert!(args.verbose);
    }

    #[test]
    fn parses_short_flags() {
        let args = HattoolsArgs::parse_from(["config-hattools", "-a", "-v"]);
        assert!(args.all);
        assert!(args.verbose);
    }

    #[test]
    fn run_always_returns_zero() {
        assert_eq!(run(&HattoolsArgs::default()), 0);
        assert_eq!(run(&HattoolsArgs { all: true, verbose: true }), 0);
    }
}
