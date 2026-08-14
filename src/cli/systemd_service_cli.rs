//! CLI wrapper mirroring `configurator.systemd_service:main` (a `config-systemd-service` tool).
use clap::Parser;

use crate::api::systemd_service::SystemdServiceManager;
use crate::api::wifi::SystemCommandRunner;

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-systemd-service", about = "SystemD Service Management Tool")]
pub struct SystemdServiceArgs {
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Enable debug logging
    #[arg(long = "very-verbose")]
    pub very_verbose: bool,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Name of the service to manage
    pub service: Option<String>,

    /// Enable the service
    #[arg(long, group = "action")]
    pub enable: bool,

    /// Disable the service
    #[arg(long, group = "action")]
    pub disable: bool,

    /// Start the service
    #[arg(long, group = "action")]
    pub start: bool,

    /// Stop the service
    #[arg(long, group = "action")]
    pub stop: bool,

    /// Restart the service
    #[arg(long, group = "action")]
    pub restart: bool,

    /// Reload the service
    #[arg(long, group = "action")]
    pub reload: bool,

    /// Get service status
    #[arg(long, group = "action")]
    pub status: bool,

    /// Check if service is active
    #[arg(long = "is-active", group = "action")]
    pub is_active: bool,

    /// Check if service is enabled
    #[arg(long = "is-enabled", group = "action")]
    pub is_enabled: bool,

    /// List services
    #[arg(long, group = "action")]
    pub list: bool,

    /// Reload systemd daemon
    #[arg(long = "daemon-reload", group = "action")]
    pub daemon_reload: bool,

    /// Pattern to filter services when listing
    #[arg(long)]
    pub pattern: Option<String>,
}

/// Run the systemd-service CLI. Returns the process exit code.
pub fn run(args: &SystemdServiceArgs) -> i32 {
    let actions_requiring_service = [
        args.enable, args.disable, args.start, args.stop, args.restart, args.reload, args.status, args.is_active, args.is_enabled,
    ];

    if actions_requiring_service.iter().any(|a| *a) && args.service.is_none() {
        eprintln!("Error: Service name is required for this action");
        return 1;
    }

    let runner = SystemCommandRunner;
    let manager = SystemdServiceManager::new(&runner);
    let service = args.service.as_deref().unwrap_or_default();

    if args.enable {
        return print_message_result(manager.enable(service), args.json);
    }
    if args.disable {
        return print_message_result(manager.disable(service), args.json);
    }
    if args.start {
        return print_message_result(manager.start(service), args.json);
    }
    if args.stop {
        return print_message_result(manager.stop(service), args.json);
    }
    if args.restart {
        return print_message_result(manager.restart(service), args.json);
    }
    if args.reload {
        return print_message_result(manager.reload(service), args.json);
    }
    if args.status {
        let (success, status) = manager.status(service);
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "success": success, "status": status })).unwrap()
            );
        } else {
            println!("Service: {}", status.service_name);
            println!("Active: {}", status.active);
            println!("Enabled: {}", status.enabled);
            if status.status_available {
                println!("Status Output:\n{}", status.status_output);
            } else {
                println!("Failed to get status: {}", status.status_output);
            }
        }
        return if success { 0 } else { 1 };
    }
    if args.is_active {
        let result = manager.is_active(service);
        print_bool_result(result, args.json, "active", "inactive");
        return 0;
    }
    if args.is_enabled {
        let result = manager.is_enabled(service);
        print_bool_result(result, args.json, "enabled", "disabled");
        return 0;
    }
    if args.list {
        let (success, services) = manager.list_services(args.pattern.as_deref());
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "success": success, "services": services })).unwrap()
            );
        } else if success {
            if services.is_empty() {
                println!("No services found");
            } else {
                println!("{:<30} {:<10} {:<10} {:<10} {}", "NAME", "LOAD", "ACTIVE", "SUB", "DESCRIPTION");
                println!("{}", "-".repeat(80));
                for s in &services {
                    println!("{:<30} {:<10} {:<10} {:<10} {}", s.name, s.load, s.active, s.sub, s.description);
                }
            }
        } else {
            println!("Failed to list services");
        }
        return if success { 0 } else { 1 };
    }
    if args.daemon_reload {
        return print_message_result(manager.daemon_reload(), args.json);
    }

    eprintln!("Error: no action specified");
    1
}

fn print_message_result((success, message): (bool, String), json: bool) -> i32 {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "success": success, "message": message })).unwrap()
        );
    } else {
        println!("{}", message);
    }
    if success {
        0
    } else {
        1
    }
}

fn print_bool_result(result: bool, json: bool, when_true: &str, when_false: &str) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "result": result, "success": true })).unwrap()
        );
    } else {
        println!("{}", if result { when_true } else { when_false });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = SystemdServiceArgs::parse_from(["config-systemd-service"]);
        assert!(!args.verbose);
        assert!(!args.json);
        assert!(args.service.is_none());
    }

    #[test]
    fn parses_enable_with_service_name() {
        let args = SystemdServiceArgs::parse_from(["config-systemd-service", "demo", "--enable"]);
        assert_eq!(args.service.as_deref(), Some("demo"));
        assert!(args.enable);
    }

    #[test]
    fn parses_list_with_pattern_and_json() {
        let args = SystemdServiceArgs::parse_from(["config-systemd-service", "--list", "--pattern", "hifi", "--json"]);
        assert!(args.list);
        assert_eq!(args.pattern.as_deref(), Some("hifi"));
        assert!(args.json);
    }

    #[test]
    fn run_requires_service_name_for_service_actions() {
        let mut args = SystemdServiceArgs::default();
        args.enable = true;
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn run_returns_error_when_no_action_specified() {
        let args = SystemdServiceArgs::default();
        assert_eq!(run(&args), 1);
    }

    #[test]
    fn parses_verbose_and_very_verbose_flags() {
        let args = SystemdServiceArgs::parse_from(["config-systemd-service", "-v", "--very-verbose"]);
        assert!(args.verbose);
        assert!(args.very_verbose);
    }
}
