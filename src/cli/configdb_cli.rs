//! CLI wrapper mirroring `configurator.configdb:main` (the `config-configdb` tool).
use clap::Parser;

use crate::api::configdb::ConfigDb;

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-configdb", about = "Manage HiFiBerry OS configuration database")]
pub struct ConfigdbArgs {
    /// Get a value from the configuration
    #[arg(long, value_name = "KEY")]
    pub get: Option<String>,

    /// Set a key/value pair
    #[arg(long, num_args = 2, value_names = ["KEY", "VALUE"])]
    pub set: Option<Vec<String>>,

    /// Delete a key
    #[arg(long, value_name = "KEY")]
    pub delete: Option<String>,

    /// List all keys
    #[arg(long)]
    pub list: bool,

    /// Dump all key/value pairs
    #[arg(long)]
    pub dump: bool,

    /// Filter keys by prefix (for use with --list or --dump)
    #[arg(long)]
    pub prefix: Option<String>,

    /// Default value if key does not exist (for use with --get)
    #[arg(long)]
    pub default: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Legacy command (get, set, delete, list, dump) and its arguments
    pub command: Option<String>,
    pub args: Vec<String>,
}

/// Run the configdb CLI. Returns the process exit code (mirrors Python's `main()`).
pub fn run(args: &ConfigdbArgs) -> i32 {
    let has_new_style = args.get.is_some() || args.set.is_some() || args.delete.is_some() || args.list || args.dump;
    if has_new_style && args.command.is_some() {
        eprintln!("Cannot combine option commands (--get/--set/--delete/--list/--dump) with legacy positional commands");
        return 2;
    }

    let mut db = ConfigDb::open_default();

    if let Some(key) = &args.get {
        return match db.get(key, args.default.as_deref(), false) {
            Some(value) => {
                println!("{value}");
                0
            }
            None => 1,
        };
    }
    if let Some(pair) = &args.set {
        let (key, value) = (&pair[0], &pair[1]);
        if !db.set(key, value, false) {
            eprintln!("Failed to set {key}");
            return 1;
        }
        return 0;
    }
    if let Some(key) = &args.delete {
        if !db.delete(key) {
            eprintln!("Failed to delete {key}");
            return 1;
        }
        return 0;
    }
    if args.list {
        for key in db.list_keys(args.prefix.as_deref()) {
            println!("{key}");
        }
        return 0;
    }
    if args.dump {
        for (key, value) in db.get_all(args.prefix.as_deref()) {
            println!("{key}={value}");
        }
        return 0;
    }

    // Legacy positional syntax.
    if let Some(command) = &args.command {
        match command.as_str() {
            "get" if !args.args.is_empty() => {
                let key = &args.args[0];
                let default = args.args.get(1).map(|s| s.as_str());
                return match db.get(key, default, false) {
                    Some(value) => {
                        println!("{value}");
                        0
                    }
                    None => 1,
                };
            }
            "set" if args.args.len() >= 2 => {
                let (key, value) = (&args.args[0], &args.args[1]);
                if !db.set(key, value, false) {
                    eprintln!("Failed to set {key}");
                    return 1;
                }
                return 0;
            }
            "delete" if !args.args.is_empty() => {
                let key = &args.args[0];
                if !db.delete(key) {
                    eprintln!("Failed to delete {key}");
                    return 1;
                }
                return 0;
            }
            "list" => {
                let prefix = args.args.first().map(|s| s.as_str());
                for key in db.list_keys(prefix) {
                    println!("{key}");
                }
                return 0;
            }
            "dump" => {
                let prefix = args.args.first().map(|s| s.as_str());
                for (key, value) in db.get_all(prefix) {
                    println!("{key}={value}");
                }
                return 0;
            }
            _ => {}
        }
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_flag() {
        let args = ConfigdbArgs::parse_from(["config-configdb", "--get", "foo"]);
        assert_eq!(args.get, Some("foo".to_string()));
    }

    #[test]
    fn parses_set_flag_with_two_values() {
        let args = ConfigdbArgs::parse_from(["config-configdb", "--set", "foo", "bar"]);
        assert_eq!(args.set, Some(vec!["foo".to_string(), "bar".to_string()]));
    }

    #[test]
    fn run_rejects_combining_new_style_and_legacy() {
        let args = ConfigdbArgs::parse_from(["config-configdb", "--list", "legacy-cmd"]);
        assert_eq!(run(&args), 2);
    }

    #[test]
    fn run_set_then_get_roundtrip() {
        let key = "cli.roundtrip.test.key".to_string();
        let set_args = ConfigdbArgs { set: Some(vec![key.clone(), "value1".to_string()]), ..Default::default() };
        assert_eq!(run(&set_args), 0);

        let get_args = ConfigdbArgs { get: Some(key.clone()), ..Default::default() };
        assert_eq!(run(&get_args), 0);

        let delete_args = ConfigdbArgs { delete: Some(key), ..Default::default() };
        assert_eq!(run(&delete_args), 0);
    }

    #[test]
    fn run_get_missing_key_returns_one() {
        let args = ConfigdbArgs { get: Some("cli.missing.key.xyz".to_string()), ..Default::default() };
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_list_returns_zero() {
        let args = ConfigdbArgs { list: true, ..Default::default() };
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn run_legacy_get_syntax() {
        let key = "cli.legacy.test.key".to_string();
        let set_args = ConfigdbArgs { command: Some("set".to_string()), args: vec![key.clone(), "v".to_string()], ..Default::default() };
        assert_eq!(run(&set_args), 0);

        let get_args = ConfigdbArgs { command: Some("get".to_string()), args: vec![key], ..Default::default() };
        assert_eq!(run(&get_args), 0);
    }

    #[test]
    fn run_no_command_returns_one() {
        assert_eq!(run(&ConfigdbArgs::default()), 1);
    }
}
