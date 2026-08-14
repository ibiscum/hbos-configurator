//! Port of `configurator/sambamount.py` (SMB/CIFS share mount configuration
//! and management).
//!
//! Config persistence is abstracted behind [`ConfigDb`] (no persistent
//! ConfigDB implementation has been ported yet) and hardware/process access
//! behind [`CommandRunner`], so both can be exercised in tests the same way
//! Python patches `ConfigDB`/`subprocess.run`/`shutil.which`. `/proc/mounts`
//! content is passed in explicitly rather than read internally, keeping the
//! mount-status logic pure and testable.
use std::process::Command;

use serde::Serialize;

pub const MAX_MOUNT_SLOTS: usize = 256;

/// Key/value configuration storage abstraction (mirrors `configurator.configdb.ConfigDB`).
pub trait ConfigDb: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: &str, value: &str);
    fn delete(&mut self, key: &str);
}

/// Simple in-memory `ConfigDb`, used as the default until a persistent
/// ConfigDB implementation is ported.
#[derive(Debug, Default)]
pub struct MemoryConfigDb {
    data: std::collections::HashMap<String, String>,
    pub deleted: Vec<String>,
}

impl ConfigDb for MemoryConfigDb {
    fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_string(), value.to_string());
    }

    fn delete(&mut self, key: &str) {
        self.deleted.push(key.to_string());
        self.data.remove(key);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

/// Runs external commands and checks their availability (mirrors
/// `shutil.which`/`subprocess.run`).
pub trait CommandRunner: Send + Sync {
    fn which(&self, cmd: &str) -> bool;
    fn run(&self, args: &[&str]) -> Option<CommandOutput>;
}

/// Real implementation that spawns processes via [`std::process::Command`].
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn which(&self, cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    fn run(&self, args: &[&str]) -> Option<CommandOutput> {
        let (program, rest) = args.split_first()?;
        Command::new(program).args(rest).output().ok().map(|out| CommandOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MountConfig {
    pub id: usize,
    pub server: String,
    pub share: String,
    pub mountpoint: String,
    pub user: String,
    pub password: String,
    pub version: String,
    pub options: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MountConfigDisplay {
    pub id: usize,
    pub server: String,
    pub share: String,
    pub mountpoint: String,
    pub user: String,
    pub version: String,
    pub options: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MountStatus {
    #[serde(flatten)]
    pub config: MountConfigDisplay,
    pub mounted: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MountAllResults {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
}

fn iter_config_indices(db: &dyn ConfigDb) -> Vec<usize> {
    (1..=MAX_MOUNT_SLOTS)
        .filter(|index| db.get(&format!("smbmount.{index}.server")).is_some())
        .collect()
}

/// Read all mount configurations, including passwords.
pub fn read_mount_config(db: &dyn ConfigDb) -> Vec<MountConfig> {
    iter_config_indices(db)
        .into_iter()
        .filter_map(|index| {
            let prefix = format!("smbmount.{index}");
            let server = db.get(&format!("{prefix}.server"))?;
            Some(MountConfig {
                id: index,
                server,
                share: db.get(&format!("{prefix}.share")).unwrap_or_default(),
                mountpoint: db.get(&format!("{prefix}.mountpoint")).unwrap_or_default(),
                user: db.get(&format!("{prefix}.user")).unwrap_or_default(),
                password: db.get(&format!("{prefix}.password")).unwrap_or_default(),
                version: db.get(&format!("{prefix}.version")).unwrap_or_default(),
                options: db.get(&format!("{prefix}.options")).unwrap_or_default(),
            })
        })
        .collect()
}

/// Read mount configurations for display/listing; passwords are never
/// included so they can't leak into log or print output.
pub fn read_mount_config_for_display(db: &dyn ConfigDb) -> Vec<MountConfigDisplay> {
    iter_config_indices(db)
        .into_iter()
        .filter_map(|index| {
            let prefix = format!("smbmount.{index}");
            let server = db.get(&format!("{prefix}.server"))?;
            Some(MountConfigDisplay {
                id: index,
                server,
                share: db.get(&format!("{prefix}.share")).unwrap_or_default(),
                mountpoint: db.get(&format!("{prefix}.mountpoint")).unwrap_or_default(),
                user: db.get(&format!("{prefix}.user")).unwrap_or_default(),
                version: db.get(&format!("{prefix}.version")).unwrap_or_default(),
                options: db.get(&format!("{prefix}.options")).unwrap_or_default(),
            })
        })
        .collect()
}

/// Overwrite all mount configurations, clearing existing (possibly sparse)
/// slots first and rewriting sequentially starting at index 1.
pub fn write_mount_config(db: &mut dyn ConfigDb, mounts: &[MountConfig]) -> bool {
    for index in iter_config_indices(db) {
        let prefix = format!("smbmount.{index}");
        for field in ["server", "share", "mountpoint", "user", "password", "version", "options"] {
            db.delete(&format!("{prefix}.{field}"));
        }
    }

    for (i, mount) in mounts.iter().enumerate() {
        let prefix = format!("smbmount.{}", i + 1);
        db.set(&format!("{prefix}.server"), &mount.server);
        db.set(&format!("{prefix}.share"), &mount.share);
        db.set(&format!("{prefix}.mountpoint"), &mount.mountpoint);
        db.set(&format!("{prefix}.user"), &mount.user);
        db.set(&format!("{prefix}.password"), &mount.password);
        db.set(&format!("{prefix}.version"), &mount.version);
        db.set(&format!("{prefix}.options"), &mount.options);
    }

    true
}

/// Add a mount configuration. Returns `Err` if a configuration for the same
/// server/share already exists.
pub fn add_mount_config(
    db: &mut dyn ConfigDb,
    server: &str,
    share: &str,
    mountpoint: Option<&str>,
    user: Option<&str>,
    password: Option<&str>,
    version: Option<&str>,
    options: Option<&str>,
) -> Result<(), String> {
    let mounts = read_mount_config(db);
    if mounts.iter().any(|m| m.server == server && m.share == share) {
        return Err(format!("Mount configuration for {server}/{share} already exists"));
    }

    let mountpoint = mountpoint
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("/data/{server}-{share}"));

    let mut next_id = 1;
    while db.get(&format!("smbmount.{next_id}.server")).is_some() {
        next_id += 1;
    }
    let prefix = format!("smbmount.{next_id}");
    db.set(&format!("{prefix}.server"), server);
    db.set(&format!("{prefix}.share"), share);
    db.set(&format!("{prefix}.mountpoint"), &mountpoint);
    db.set(&format!("{prefix}.user"), user.unwrap_or(""));
    db.set(&format!("{prefix}.password"), password.unwrap_or(""));
    db.set(&format!("{prefix}.version"), version.unwrap_or(""));
    db.set(&format!("{prefix}.options"), options.unwrap_or(""));

    Ok(())
}

/// Remove a mount configuration. Returns the mountpoint it used to occupy.
pub fn remove_mount_config(db: &mut dyn ConfigDb, server: &str, share: &str) -> Result<String, String> {
    let mounts = read_mount_config(db);
    let Some(removed) = mounts.iter().find(|m| m.server == server && m.share == share) else {
        return Err(format!("Mount configuration for {server}/{share} not found"));
    };
    let mountpoint = removed.mountpoint.clone();

    let remaining: Vec<MountConfig> = mounts.into_iter().filter(|m| !(m.server == server && m.share == share)).collect();
    write_mount_config(db, &remaining);
    Ok(mountpoint)
}

/// Find a mount configuration by server and share name.
pub fn find_mount_by_server_share(db: &dyn ConfigDb, server: &str, share: &str) -> Option<MountConfig> {
    read_mount_config(db).into_iter().find(|m| m.server == server && m.share == share)
}

/// Whether `mountpoint` shows up as an active mount in `/proc/mounts` content.
pub fn is_mounted(mountpoint: &str, proc_mounts: &str) -> bool {
    if mountpoint.is_empty() {
        return false;
    }
    for line in proc_mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == mountpoint {
            return true;
        }
    }
    false
}

/// List all configured mounts (without passwords), annotated with current
/// mount status.
pub fn list_configured_mounts(db: &dyn ConfigDb, proc_mounts: &str) -> Vec<MountStatus> {
    read_mount_config_for_display(db)
        .into_iter()
        .map(|config| {
            let mounted = if config.mountpoint.is_empty() {
                false
            } else {
                is_mounted(&config.mountpoint, proc_mounts)
            };
            MountStatus { config, mounted }
        })
        .collect()
}

fn version_to_vers_option(version: &str) -> Option<&'static str> {
    match version {
        "SMB1" => Some("vers=1.0"),
        "SMB2" => Some("vers=2.1"),
        "SMB3" => Some("vers=3.0"),
        _ => None,
    }
}

fn write_credentials_file(username: &str, password: &str) -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hbos-sambamount-{}-{}.cred",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&path, format!("username={username}\npassword={password}\n"))?;
    Ok(path)
}

fn safe_unlink(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

/// Categorize a failed `mount` command's stderr into a user-facing message.
fn categorize_mount_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied") || lower.contains("access denied") {
        format!("Authentication failed: {stderr}")
    } else if lower.contains("no such file or directory") || lower.contains("not found") {
        format!("Share not found: {stderr}")
    } else if lower.contains("network is unreachable") || lower.contains("no route to host") {
        format!("Network connection failed: {stderr}")
    } else if lower.contains("connection refused") || lower.contains("connection timed out") {
        format!("SMB service unavailable: {stderr}")
    } else if stderr.contains("mount error(13)") {
        format!("Permission denied: {stderr}")
    } else if stderr.contains("mount error(2)") {
        format!("Share does not exist: {stderr}")
    } else if stderr.contains("mount error(112)") {
        format!("Host unreachable: {stderr}")
    } else if stderr.contains("mount error(115)") {
        format!("Connection timeout: {stderr}")
    } else {
        format!("Mount failed: {stderr}")
    }
}

/// Categorize a failed `umount` command's stderr into a user-facing message.
fn categorize_unmount_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("target is busy") || lower.contains("device is busy") {
        format!("Device busy: {stderr}")
    } else if lower.contains("not mounted") || lower.contains("not found") {
        format!("Not mounted: {stderr}")
    } else if lower.contains("permission denied") {
        format!("Permission denied: {stderr}")
    } else {
        format!("Unmount failed: {stderr}")
    }
}

/// Mount a CIFS share. Credentials are written to a temporary file rather
/// than passed as command-line arguments to avoid exposing secrets.
#[allow(clippy::too_many_arguments)]
pub fn mount_cifs_share(
    runner: &dyn CommandRunner,
    proc_mounts: &str,
    server: &str,
    share: &str,
    mountpoint: &str,
    username: Option<&str>,
    password: Option<&str>,
    version: Option<&str>,
    options: Option<&str>,
) -> Result<(), String> {
    if !runner.which("mount") {
        return Err("mount command not found".to_string());
    }

    if !std::path::Path::new(mountpoint).exists() {
        std::fs::create_dir_all(mountpoint).map_err(|e| format!("Error creating mountpoint {mountpoint}: {e}"))?;
    }

    if is_mounted(mountpoint, proc_mounts) {
        return Ok(());
    }

    let mut mount_opts = Vec::new();
    let mut credentials_path: Option<std::path::PathBuf> = None;

    if username.is_some() || password.is_some() {
        let path = write_credentials_file(username.unwrap_or(""), password.unwrap_or(""))
            .map_err(|e| format!("Error creating temporary credentials file: {e}"))?;
        mount_opts.push(format!("credentials={}", path.display()));
        credentials_path = Some(path);
    }

    if let Some(v) = version.and_then(version_to_vers_option) {
        mount_opts.push(v.to_string());
    }

    if let Some(opts) = options {
        mount_opts.extend(opts.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()));
    }

    let source = format!("//{server}/{share}");
    let opts_joined = mount_opts.join(",");
    let args: Vec<&str> = vec!["mount", "-t", "cifs", &source, mountpoint, "-o", &opts_joined];
    let result = runner.run(&args);

    if let Some(path) = &credentials_path {
        safe_unlink(path);
    }

    match result {
        Some(output) if output.success() => Ok(()),
        Some(output) => Err(categorize_mount_error(output.stderr.trim())),
        None => Err("Mount command failed to execute".to_string()),
    }
}

/// Unmount a share, optionally retrying with a lazy unmount if the target is busy.
pub fn unmount_share(runner: &dyn CommandRunner, proc_mounts: &str, mountpoint: &str, lazy_fallback: bool) -> Result<(), String> {
    if !runner.which("umount") {
        return Err("umount command not found".to_string());
    }

    if !is_mounted(mountpoint, proc_mounts) {
        return Ok(());
    }

    let result = runner.run(&["umount", mountpoint]);
    match result {
        Some(output) if output.success() => Ok(()),
        Some(output) => {
            let stderr = output.stderr.trim();
            let lower = stderr.to_lowercase();
            let busy = lower.contains("target is busy") || lower.contains("device is busy");
            if lazy_fallback && busy {
                match runner.run(&["umount", "-l", mountpoint]) {
                    Some(lazy) if lazy.success() => Ok(()),
                    Some(lazy) => Err(format!("Lazy unmount also failed: {}", lazy.stderr.trim())),
                    None => Err("Lazy unmount failed to execute".to_string()),
                }
            } else {
                Err(categorize_unmount_error(stderr))
            }
        }
        None => Err("Unmount command failed to execute".to_string()),
    }
}

/// Mount all shares defined in the configuration database.
pub fn mount_all_shares(runner: &dyn CommandRunner, proc_mounts: &str, mounts: &[MountConfig]) -> MountAllResults {
    let mut results = MountAllResults::default();
    for mount in mounts {
        let user = (!mount.user.is_empty()).then_some(mount.user.as_str());
        let password = (!mount.password.is_empty()).then_some(mount.password.as_str());
        let version = (!mount.version.is_empty()).then_some(mount.version.as_str());
        let options = (!mount.options.is_empty()).then_some(mount.options.as_str());

        match mount_cifs_share(runner, proc_mounts, &mount.server, &mount.share, &mount.mountpoint, user, password, version, options) {
            Ok(()) => results.succeeded.push(format!("{}/{} at {}", mount.server, mount.share, mount.mountpoint)),
            Err(e) => results.failed.push(format!("{}/{} at {}: {}", mount.server, mount.share, mount.mountpoint, e)),
        }
    }
    results
}

/// Mount a specific SMB share by server/share name, verifying afterward.
pub fn mount_smb_share(runner: &dyn CommandRunner, proc_mounts: &str, mounts: &[MountConfig], server: &str, share: &str) -> Result<(), String> {
    let Some(target) = mounts.iter().find(|m| m.server == server && m.share == share) else {
        return Err(format!("Mount configuration for {server}/{share} not found"));
    };

    if is_mounted(&target.mountpoint, proc_mounts) {
        return Ok(());
    }

    let user = (!target.user.is_empty()).then_some(target.user.as_str());
    let password = (!target.password.is_empty()).then_some(target.password.as_str());
    let version = (!target.version.is_empty()).then_some(target.version.as_str());
    let options = (!target.options.is_empty()).then_some(target.options.as_str());

    mount_cifs_share(runner, proc_mounts, server, share, &target.mountpoint, user, password, version, options)
}

/// Unmount a specific SMB share by server/share name.
pub fn unmount_smb_share(runner: &dyn CommandRunner, proc_mounts: &str, mounts: &[MountConfig], server: &str, share: &str) -> Result<(), String> {
    let Some(target) = mounts.iter().find(|m| m.server == server && m.share == share) else {
        return Err(format!("Mount configuration for {server}/{share} not found"));
    };

    if !is_mounted(&target.mountpoint, proc_mounts) {
        return Ok(());
    }

    unmount_share(runner, proc_mounts, &target.mountpoint, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn db_with(entries: &[(&str, &str)]) -> MemoryConfigDb {
        let mut db = MemoryConfigDb::default();
        for (k, v) in entries {
            db.set(k, v);
        }
        db
    }

    fn mount(id: usize, server: &str, share: &str) -> MountConfig {
        MountConfig {
            id,
            server: server.to_string(),
            share: share.to_string(),
            mountpoint: format!("/mnt/{share}"),
            user: String::new(),
            password: String::new(),
            version: String::new(),
            options: String::new(),
        }
    }

    #[test]
    fn read_mount_config_handles_sparse_indices() {
        let db = db_with(&[
            ("smbmount.1.server", "srv1"),
            ("smbmount.1.share", "music"),
            ("smbmount.1.mountpoint", "/mnt/music"),
            ("smbmount.1.user", "u1"),
            ("smbmount.1.password", "p1"),
            ("smbmount.1.version", "SMB3"),
            ("smbmount.1.options", ""),
            ("smbmount.3.server", "srv3"),
            ("smbmount.3.share", "backup"),
            ("smbmount.3.mountpoint", "/mnt/backup"),
            ("smbmount.3.user", "u3"),
            ("smbmount.3.password", "p3"),
            ("smbmount.3.version", "SMB2"),
            ("smbmount.3.options", "ro"),
        ]);

        let mounts = read_mount_config(&db);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].id, 1);
        assert_eq!(mounts[1].id, 3);
        assert_eq!(mounts[1].server, "srv3");
    }

    #[test]
    fn write_mount_config_clears_sparse_existing_entries() {
        let mut db = db_with(&[
            ("smbmount.1.server", "old1"),
            ("smbmount.1.share", "a"),
            ("smbmount.3.server", "old3"),
            ("smbmount.3.share", "c"),
        ]);

        let success = write_mount_config(
            &mut db,
            &[MountConfig {
                id: 1,
                server: "new".to_string(),
                share: "media".to_string(),
                mountpoint: "/mnt/media".to_string(),
                user: String::new(),
                password: String::new(),
                version: String::new(),
                options: String::new(),
            }],
        );

        assert!(success);
        assert!(db.deleted.contains(&"smbmount.1.server".to_string()));
        assert!(db.deleted.contains(&"smbmount.3.server".to_string()));
        assert_eq!(db.get("smbmount.1.server"), Some("new".to_string()));
    }

    struct StubRunner {
        which: bool,
        calls: Mutex<Vec<Vec<String>>>,
        response: CommandOutput,
    }

    impl CommandRunner for StubRunner {
        fn which(&self, _cmd: &str) -> bool {
            self.which
        }
        fn run(&self, args: &[&str]) -> Option<CommandOutput> {
            self.calls.lock().unwrap().push(args.iter().map(|s| s.to_string()).collect());
            Some(self.response.clone())
        }
    }

    #[test]
    fn mount_cifs_share_uses_credentials_file_not_password_arg() {
        let tmp = tempfile::tempdir().unwrap();
        let mountpoint = tmp.path().to_str().unwrap();
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };

        let result = mount_cifs_share(
            &runner,
            "",
            "server",
            "share",
            mountpoint,
            Some("alice"),
            Some("secret"),
            Some("SMB3"),
            Some("rw,nosuid"),
        );

        assert!(result.is_ok());
        let calls = runner.calls.lock().unwrap();
        let cmd = &calls[0];
        let dash_o = cmd.iter().position(|a| a == "-o").unwrap();
        let mount_opts = &cmd[dash_o + 1];
        assert!(mount_opts.contains("credentials="));
        assert!(!mount_opts.contains("password=secret"));
        assert!(!mount_opts.contains("username=alice"));
        assert!(mount_opts.contains("vers=3.0"));
        assert!(mount_opts.contains("rw"));
    }

    #[test]
    fn mount_cifs_share_fails_when_mount_command_missing() {
        let runner = StubRunner {
            which: false,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        let result = mount_cifs_share(&runner, "", "server", "share", "/mnt/x", None, None, None, None);
        assert_eq!(result, Err("mount command not found".to_string()));
    }

    #[test]
    fn mount_cifs_share_returns_ok_when_already_mounted() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        let tmp = tempfile::tempdir().unwrap();
        let mountpoint = tmp.path().to_str().unwrap();
        let proc_mounts = format!("dev {mountpoint} cifs rw 0 0");

        let result = mount_cifs_share(&runner, &proc_mounts, "server", "share", mountpoint, None, None, None, None);
        assert!(result.is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn mount_cifs_share_categorizes_permission_denied_error() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 1, stdout: String::new(), stderr: "mount error: Permission denied".to_string() },
        };
        let tmp = tempfile::tempdir().unwrap();
        let mountpoint = tmp.path().to_str().unwrap();
        let result = mount_cifs_share(&runner, "", "server", "share", mountpoint, None, None, None, None);
        assert!(result.unwrap_err().starts_with("Authentication failed"));
    }

    #[test]
    fn is_mounted_matches_exact_mountpoint() {
        let proc_mounts = "//server/share /mnt/music cifs rw,relatime 0 0\n/dev/sda1 / ext4 rw 0 0\n";
        assert!(is_mounted("/mnt/music", proc_mounts));
        assert!(!is_mounted("/mnt/other", proc_mounts));
        assert!(!is_mounted("", proc_mounts));
    }

    #[test]
    fn unmount_share_returns_ok_when_not_mounted() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        assert!(unmount_share(&runner, "", "/mnt/music", false).is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn unmount_share_falls_back_to_lazy_when_busy() {
        struct LazyRunner {
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl CommandRunner for LazyRunner {
            fn which(&self, _cmd: &str) -> bool {
                true
            }
            fn run(&self, args: &[&str]) -> Option<CommandOutput> {
                self.calls.lock().unwrap().push(args.iter().map(|s| s.to_string()).collect());
                if args.contains(&"-l") {
                    Some(CommandOutput { status: 0, stdout: String::new(), stderr: String::new() })
                } else {
                    Some(CommandOutput { status: 1, stdout: String::new(), stderr: "target is busy".to_string() })
                }
            }
        }
        let runner = LazyRunner { calls: Mutex::new(Vec::new()) };
        let proc_mounts = "dev /mnt/music cifs rw 0 0";
        let result = unmount_share(&runner, proc_mounts, "/mnt/music", true);
        assert!(result.is_ok());
        assert_eq!(runner.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn unmount_share_reports_busy_without_lazy_fallback() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 1, stdout: String::new(), stderr: "target is busy".to_string() },
        };
        let proc_mounts = "dev /mnt/music cifs rw 0 0";
        let result = unmount_share(&runner, proc_mounts, "/mnt/music", false);
        assert!(result.unwrap_err().starts_with("Device busy"));
    }

    #[test]
    fn add_mount_config_rejects_duplicate() {
        let mut db = MemoryConfigDb::default();
        add_mount_config(&mut db, "srv", "share", None, None, None, None, None).unwrap();
        let err = add_mount_config(&mut db, "srv", "share", None, None, None, None, None).unwrap_err();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn add_mount_config_defaults_mountpoint() {
        let mut db = MemoryConfigDb::default();
        add_mount_config(&mut db, "srv", "share", None, Some("bob"), Some("pw"), Some("SMB3"), None).unwrap();
        let mounts = read_mount_config(&db);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mountpoint, "/data/srv-share");
        assert_eq!(mounts[0].user, "bob");
        assert_eq!(mounts[0].password, "pw");
    }

    #[test]
    fn remove_mount_config_removes_matching_entry() {
        let mut db = MemoryConfigDb::default();
        add_mount_config(&mut db, "srv", "share", Some("/mnt/x"), None, None, None, None).unwrap();
        let mountpoint = remove_mount_config(&mut db, "srv", "share").unwrap();
        assert_eq!(mountpoint, "/mnt/x");
        assert!(read_mount_config(&db).is_empty());
    }

    #[test]
    fn remove_mount_config_errors_when_not_found() {
        let mut db = MemoryConfigDb::default();
        let err = remove_mount_config(&mut db, "srv", "share").unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn find_mount_by_server_share_locates_entry() {
        let mut db = MemoryConfigDb::default();
        add_mount_config(&mut db, "srv", "share", None, None, None, None, None).unwrap();
        assert!(find_mount_by_server_share(&db, "srv", "share").is_some());
        assert!(find_mount_by_server_share(&db, "srv", "other").is_none());
    }

    #[test]
    fn list_configured_mounts_annotates_mount_status() {
        let mut db = MemoryConfigDb::default();
        add_mount_config(&mut db, "srv", "share", Some("/mnt/music"), None, None, None, None).unwrap();
        let proc_mounts = "dev /mnt/music cifs rw 0 0";
        let statuses = list_configured_mounts(&db, proc_mounts);
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].mounted);
        assert_eq!(statuses[0].config.server, "srv");
    }

    #[test]
    fn mount_all_shares_reports_succeeded_and_failed() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        let tmp = tempfile::tempdir().unwrap();
        let mut good = mount(1, "srv", "good");
        good.mountpoint = tmp.path().to_str().unwrap().to_string();
        let results = mount_all_shares(&runner, "", &[good]);
        assert_eq!(results.succeeded.len(), 1);
        assert!(results.failed.is_empty());
    }

    #[test]
    fn mount_smb_share_errors_when_config_missing() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        let err = mount_smb_share(&runner, "", &[], "srv", "share").unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn unmount_smb_share_returns_ok_when_not_mounted() {
        let runner = StubRunner {
            which: true,
            calls: Mutex::new(Vec::new()),
            response: CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        };
        let m = mount(1, "srv", "share");
        assert!(unmount_smb_share(&runner, "", &[m], "srv", "share").is_ok());
    }
}
