//! CLI wrapper mirroring `configurator.systeminfo:main` (the `config-systeminfo` tool).
use clap::Parser;

use crate::api::systeminfo::SystemInfo;

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-systeminfo", about = "HiFiBerry System Information")]
pub struct SystemInfoArgs {
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Output as JSON instead of simple text
    #[arg(long)]
    pub json: bool,
}

/// Run the systeminfo CLI: prints either a flat JSON object or simple text output.
pub fn run(args: &SystemInfoArgs) {
    if args.verbose {
        tracing::debug!("verbose logging enabled for config-systeminfo");
    }

    let info = SystemInfo::new();
    if args.json {
        let flat: serde_json::Map<String, serde_json::Value> = info
            .get_flat_info_dict()
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::Value::Object(flat)).unwrap()
        );
    } else {
        info.print_simple_output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = SystemInfoArgs::parse_from(["config-systeminfo"]);
        assert!(!args.verbose);
        assert!(!args.json);
    }

    #[test]
    fn parses_verbose_and_json_flags() {
        let args = SystemInfoArgs::parse_from(["config-systeminfo", "--verbose", "--json"]);
        assert!(args.verbose);
        assert!(args.json);
    }

    #[test]
    fn short_verbose_flag_is_accepted() {
        let args = SystemInfoArgs::parse_from(["config-systeminfo", "-v"]);
        assert!(args.verbose);
    }
}
