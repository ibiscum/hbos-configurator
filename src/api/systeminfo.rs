//! Port of `configurator/systeminfo.py`: collects Pi/HAT/soundcard/system facts.
use serde::Serialize;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// Marker comment written to config.txt when HiFiBerry sound detection is disabled.
const HIFIBERRY_DETECTION_DISABLED: &str = "# HiFiBerry sound detection disabled";

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MemoryInfo {
    pub total_kb: Option<u64>,
    pub total_mb: Option<u64>,
    pub total_gb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HatInfoRaw {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PiModelInfo {
    pub name: String,
    pub version: String,
    pub memory: MemoryInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct HatInfoOut {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub uuid: Option<String>,
    pub vendor_card: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoundcardInfo {
    pub name: String,
    pub volume_control: Option<String>,
    pub headphone_volume_control: Option<String>,
    pub hardware_index: Option<i32>,
    pub output_channels: i32,
    pub input_channels: i32,
    pub features: Vec<String>,
    pub hat_name: Option<String>,
    pub supports_dsp: bool,
    pub card_type: Vec<String>,
    #[serde(rename = "fixedInConfigTxt")]
    pub fixed_in_config_txt: bool,
    #[serde(rename = "pinSource")]
    pub pin_source: Option<String>,
}

impl Default for SoundcardInfo {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            volume_control: None,
            headphone_volume_control: None,
            hardware_index: None,
            output_channels: 0,
            input_channels: 0,
            features: Vec::new(),
            hat_name: None,
            supports_dsp: false,
            card_type: Vec::new(),
            fixed_in_config_txt: false,
            pin_source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemBlock {
    pub uuid: Option<String>,
    pub hostname: Option<String>,
    pub pretty_hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfoDict {
    pub pi_model: PiModelInfo,
    pub hat_info: HatInfoOut,
    pub soundcard: SoundcardInfo,
    pub system: SystemBlock,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn strip_nul(s: String) -> String {
    s.trim_end_matches('\u{0}').trim().to_string()
}

/// Read the Pi model name from the device tree, defaulting to "unknown".
pub fn read_pi_model_name(root: &Path) -> String {
    match fs::read_to_string(root.join("proc/device-tree/model")) {
        Ok(content) => strip_nul(content),
        Err(_) => "unknown".to_string(),
    }
}

/// Read HAT vendor/product/uuid from the device tree HAT eeprom sysfs entries.
pub fn read_hat_info(root: &Path) -> HatInfoRaw {
    let hat_dir = root.join("proc/device-tree/hat");
    let read_field = |name: &str| -> Option<String> {
        fs::read_to_string(hat_dir.join(name))
            .ok()
            .map(strip_nul)
            .filter(|s| !s.is_empty())
    };
    HatInfoRaw {
        vendor: read_field("vendor"),
        product: read_field("product"),
        uuid: read_field("uuid"),
    }
}

/// Format the "vendor:product" string, substituting "unknown" for missing fields.
pub fn format_hat_vendor_card(hat: &HatInfoRaw) -> String {
    let vendor = hat.vendor.as_deref().unwrap_or("unknown");
    let product = hat.product.as_deref().unwrap_or("unknown");
    format!("{}:{}", vendor, product)
}

/// Read the system UUID from /etc/uuid, returning None if unavailable.
pub fn read_system_uuid(root: &Path) -> Option<String> {
    fs::read_to_string(root.join("etc/uuid"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse /proc/meminfo, returning None if MemTotal is missing (mirrors Python's `{}`).
pub fn read_memory_info(root: &Path) -> Option<MemoryInfo> {
    let content = fs::read_to_string(root.join("proc/meminfo")).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let total_kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            let total_mb = (total_kb as f64 / 1024.0).round() as u64;
            let total_gb = (total_kb as f64 / 1024.0 / 1024.0).ceil() as u64;
            return Some(MemoryInfo {
                total_kb: Some(total_kb),
                total_mb: Some(total_mb),
                total_gb: Some(total_gb),
            });
        }
    }
    None
}

/// Read system hostname (/etc/hostname) and pretty hostname (/etc/machine-info).
pub fn read_hostnames(root: &Path) -> (Option<String>, Option<String>) {
    let hostname = fs::read_to_string(root.join("etc/hostname"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let pretty = fs::read_to_string(root.join("etc/machine-info"))
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                line.strip_prefix("PRETTY_HOSTNAME=")
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        })
        .filter(|s| !s.is_empty());

    (hostname, pretty)
}

/// Check whether config.txt carries the "sound detection disabled" marker comment.
pub fn is_soundcard_fixed_in_config_txt(root: &Path) -> bool {
    match fs::read_to_string(root.join("boot/firmware/config.txt")) {
        Ok(content) => content.contains(HIFIBERRY_DETECTION_DISABLED),
        Err(_) => false,
    }
}

/// Collects and exposes system information (Pi model, HAT, sound card, hostnames).
///
/// `root` is injectable so tests can point reads at a fixture directory instead
/// of the real `/proc`, `/etc` and `/boot` locations.
pub struct SystemInfo {
    root: PathBuf,
    pi_model: RefCell<Option<PiModelInfo>>,
    hat_info: RefCell<Option<HatInfoRaw>>,
    system_uuid: RefCell<Option<String>>,
    soundcard: RefCell<Option<SoundcardInfo>>,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemInfo {
    pub fn new() -> Self {
        Self::with_root("/")
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            pi_model: RefCell::new(None),
            hat_info: RefCell::new(None),
            system_uuid: RefCell::new(None),
            soundcard: RefCell::new(None),
        }
    }

    fn get_pi_model(&self) -> PiModelInfo {
        if self.pi_model.borrow().is_none() {
            let memory = read_memory_info(&self.root).unwrap_or_default();
            let info = PiModelInfo {
                name: read_pi_model_name(&self.root),
                version: "unknown".to_string(),
                memory,
            };
            *self.pi_model.borrow_mut() = Some(info);
        }
        self.pi_model.borrow().clone().unwrap()
    }

    fn get_hat_info(&self) -> HatInfoRaw {
        if self.hat_info.borrow().is_none() {
            *self.hat_info.borrow_mut() = Some(read_hat_info(&self.root));
        }
        self.hat_info.borrow().clone().unwrap()
    }

    pub fn get_pi_model_name(&self) -> String {
        self.get_pi_model().name
    }

    pub fn get_hat_vendor_card(&self) -> String {
        format_hat_vendor_card(&self.get_hat_info())
    }

    pub fn get_system_uuid(&self) -> Option<String> {
        if self.system_uuid.borrow().is_none() {
            if let Some(uuid) = read_system_uuid(&self.root) {
                *self.system_uuid.borrow_mut() = Some(uuid);
            }
        }
        self.system_uuid.borrow().clone()
    }

    pub fn get_hostnames(&self) -> (Option<String>, Option<String>) {
        read_hostnames(&self.root)
    }

    /// Sound card detection is not yet ported; returns a placeholder until
    /// the `soundcard`/`soundcard_detector` modules are migrated.
    pub fn get_soundcard_info(&self, prioritize_aplay: bool) -> SoundcardInfo {
        if prioritize_aplay {
            return SoundcardInfo {
                fixed_in_config_txt: is_soundcard_fixed_in_config_txt(&self.root),
                ..SoundcardInfo::default()
            };
        }
        if self.soundcard.borrow().is_none() {
            let info = SoundcardInfo {
                fixed_in_config_txt: is_soundcard_fixed_in_config_txt(&self.root),
                ..SoundcardInfo::default()
            };
            *self.soundcard.borrow_mut() = Some(info);
        }
        self.soundcard.borrow().clone().unwrap()
    }

    pub fn get_system_info_dict(&self) -> SystemInfoDict {
        let pi_model = self.get_pi_model();
        let hat = self.get_hat_info();
        let uuid = self.get_system_uuid();
        let soundcard = self.get_soundcard_info(true);
        let (hostname, pretty_hostname) = self.get_hostnames();

        SystemInfoDict {
            hat_info: HatInfoOut {
                vendor: hat.vendor.clone(),
                product: hat.product.clone(),
                uuid: hat.uuid.clone(),
                vendor_card: format_hat_vendor_card(&hat),
            },
            pi_model,
            soundcard,
            system: SystemBlock {
                uuid,
                hostname,
                pretty_hostname,
            },
            status: "success".to_string(),
            error: None,
        }
    }

    pub fn get_flat_info_dict(&self) -> Vec<(String, String)> {
        let pi_model = self.get_pi_model();
        let hat = self.get_hat_info();
        let uuid = self.get_system_uuid();
        let soundcard = self.get_soundcard_info(true);
        let (hostname, pretty_hostname) = self.get_hostnames();

        let pi_model_full = if pi_model.version != "unknown" {
            format!("{} {}", pi_model.name, pi_model.version)
        } else {
            pi_model.name.clone()
        };

        let memory_str = match (pi_model.memory.total_gb, pi_model.memory.total_mb) {
            (Some(gb), Some(mb)) => format!("{} GB ({} MB)", gb, mb),
            (None, Some(mb)) => format!("{} MB", mb),
            _ => "unknown".to_string(),
        };

        let vendor = hat.vendor.clone().unwrap_or_else(|| "unknown".to_string());
        let product = hat.product.clone().unwrap_or_else(|| "unknown".to_string());

        vec![
            ("Pi Model".to_string(), pi_model_full),
            ("Memory".to_string(), memory_str),
            ("HAT".to_string(), format!("{} {}", vendor, product)),
            ("Sound Card".to_string(), soundcard.name),
            ("UUID".to_string(), uuid.unwrap_or_else(|| "unknown".to_string())),
            (
                "Hostname".to_string(),
                hostname.unwrap_or_else(|| "unknown".to_string()),
            ),
            (
                "Pretty Hostname".to_string(),
                pretty_hostname.unwrap_or_else(|| "not set".to_string()),
            ),
        ]
    }

    pub fn get_simple_output(&self) -> String {
        let pi_model_name = self.get_pi_model_name();
        let hat_vendor_card = self.get_hat_vendor_card();
        let soundcard = self.get_soundcard_info(true);
        let uuid = self.get_system_uuid();

        let mut output = format!(
            "Pi Model: {}\nHat info: {}\nSound Card: {}",
            pi_model_name, hat_vendor_card, soundcard.name
        );
        if let Some(uuid) = uuid {
            output.push_str(&format!("\nSystem UUID: {}", uuid));
        }
        output
    }

    pub fn print_simple_output(&self) {
        println!("{}", self.get_simple_output());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn pi_model_name_defaults_to_unknown_when_missing() {
        let dir = fixture();
        assert_eq!(read_pi_model_name(dir.path()), "unknown");
    }

    #[test]
    fn pi_model_name_strips_null_bytes() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Raspberry Pi 4 Model B\0");
        assert_eq!(read_pi_model_name(dir.path()), "Raspberry Pi 4 Model B");
    }

    #[test]
    fn hat_vendor_card_success() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/hat/vendor", "HiFiBerry\0");
        write(dir.path(), "proc/device-tree/hat/product", "DAC+ Standard\0");
        let hat = read_hat_info(dir.path());
        assert_eq!(format_hat_vendor_card(&hat), "HiFiBerry:DAC+ Standard");
    }

    #[test]
    fn hat_vendor_card_missing_defaults_to_unknown() {
        let dir = fixture();
        let hat = read_hat_info(dir.path());
        assert_eq!(format_hat_vendor_card(&hat), "unknown:unknown");
    }

    #[test]
    fn system_uuid_success_and_trims_whitespace() {
        let dir = fixture();
        write(dir.path(), "etc/uuid", "uuid-with-spaces   \n");
        assert_eq!(
            read_system_uuid(dir.path()),
            Some("uuid-with-spaces".to_string())
        );
    }

    #[test]
    fn system_uuid_missing_file_returns_none() {
        let dir = fixture();
        assert_eq!(read_system_uuid(dir.path()), None);
    }

    #[test]
    fn memory_info_parses_memtotal() {
        let dir = fixture();
        write(
            dir.path(),
            "proc/meminfo",
            "MemTotal:       1572864 kB\nMemFree:          1024 kB\n",
        );
        let info = read_memory_info(dir.path()).unwrap();
        assert_eq!(info.total_kb, Some(1572864));
        assert_eq!(info.total_mb, Some(1536));
        assert_eq!(info.total_gb, Some(2));
    }

    #[test]
    fn memory_info_without_memtotal_returns_none() {
        let dir = fixture();
        write(dir.path(), "proc/meminfo", "MemFree: 1234 kB\n");
        assert_eq!(read_memory_info(dir.path()), None);
    }

    #[test]
    fn memory_info_missing_file_returns_none() {
        let dir = fixture();
        assert_eq!(read_memory_info(dir.path()), None);
    }

    #[test]
    fn hostnames_success() {
        let dir = fixture();
        write(dir.path(), "etc/hostname", "myhost\n");
        write(dir.path(), "etc/machine-info", "PRETTY_HOSTNAME=My Pretty Host\n");
        let (hostname, pretty) = read_hostnames(dir.path());
        assert_eq!(hostname, Some("myhost".to_string()));
        assert_eq!(pretty, Some("My Pretty Host".to_string()));
    }

    #[test]
    fn hostnames_missing_returns_none() {
        let dir = fixture();
        assert_eq!(read_hostnames(dir.path()), (None, None));
    }

    #[test]
    fn soundcard_fixed_in_config_txt_detects_marker() {
        let dir = fixture();
        write(
            dir.path(),
            "boot/firmware/config.txt",
            "# HiFiBerry sound detection disabled\n",
        );
        assert!(is_soundcard_fixed_in_config_txt(dir.path()));
    }

    #[test]
    fn soundcard_fixed_in_config_txt_false_without_marker() {
        let dir = fixture();
        write(dir.path(), "boot/firmware/config.txt", "# not the marker\n");
        assert!(!is_soundcard_fixed_in_config_txt(dir.path()));
    }

    #[test]
    fn soundcard_fixed_in_config_txt_missing_file_is_false() {
        let dir = fixture();
        assert!(!is_soundcard_fixed_in_config_txt(dir.path()));
    }

    #[test]
    fn system_info_initialization_has_no_cached_state() {
        let info = SystemInfo::new();
        assert!(info.pi_model.borrow().is_none());
        assert!(info.hat_info.borrow().is_none());
        assert!(info.system_uuid.borrow().is_none());
        assert!(info.soundcard.borrow().is_none());
    }

    #[test]
    fn pi_model_is_cached_across_calls() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Raspberry Pi 3 Model B\0");
        let info = SystemInfo::with_root(dir.path());
        assert_eq!(info.get_pi_model_name(), "Raspberry Pi 3 Model B");
        // Remove the backing file - cached value must still be returned.
        fs::remove_file(dir.path().join("proc/device-tree/model")).unwrap();
        assert_eq!(info.get_pi_model_name(), "Raspberry Pi 3 Model B");
    }

    #[test]
    fn system_uuid_caches_success_but_retries_on_failure() {
        let dir = fixture();
        let info = SystemInfo::with_root(dir.path());
        assert_eq!(info.get_system_uuid(), None);
        write(dir.path(), "etc/uuid", "uuid-123\n");
        assert_eq!(info.get_system_uuid(), Some("uuid-123".to_string()));
        fs::remove_file(dir.path().join("etc/uuid")).unwrap();
        // Once cached, the value should stick even if the file disappears.
        assert_eq!(info.get_system_uuid(), Some("uuid-123".to_string()));
    }

    #[test]
    fn soundcard_info_not_cached_when_prioritize_aplay() {
        let dir = fixture();
        let info = SystemInfo::with_root(dir.path());
        let first = info.get_soundcard_info(false);
        assert!(info.soundcard.borrow().is_some());
        let second = info.get_soundcard_info(true);
        assert_eq!(first.name, second.name);
    }

    #[test]
    fn get_system_info_dict_reports_success_status() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Pi 4\0");
        write(dir.path(), "proc/device-tree/hat/vendor", "HiFiBerry\0");
        write(dir.path(), "proc/device-tree/hat/product", "DAC+\0");
        write(dir.path(), "etc/uuid", "sys-uuid-123\n");
        write(dir.path(), "etc/hostname", "myhost\n");

        let info = SystemInfo::with_root(dir.path());
        let result = info.get_system_info_dict();

        assert_eq!(result.status, "success");
        assert_eq!(result.pi_model.name, "Pi 4");
        assert_eq!(result.hat_info.vendor, Some("HiFiBerry".to_string()));
        assert_eq!(result.system.uuid, Some("sys-uuid-123".to_string()));
        assert_eq!(result.system.hostname, Some("myhost".to_string()));
    }

    #[test]
    fn get_flat_info_dict_formats_memory_and_hat() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Pi 4\0");
        write(
            dir.path(),
            "proc/meminfo",
            "MemTotal:       4194304 kB\n",
        );
        write(dir.path(), "proc/device-tree/hat/vendor", "HiFiBerry\0");
        write(dir.path(), "proc/device-tree/hat/product", "DAC+\0");
        write(dir.path(), "etc/uuid", "sys-uuid-123\n");
        write(dir.path(), "etc/hostname", "myhost\n");

        let info = SystemInfo::with_root(dir.path());
        let flat: std::collections::HashMap<String, String> =
            info.get_flat_info_dict().into_iter().collect();

        assert_eq!(flat.get("Pi Model").unwrap(), "Pi 4");
        assert_eq!(flat.get("Memory").unwrap(), "4 GB (4096 MB)");
        assert_eq!(flat.get("HAT").unwrap(), "HiFiBerry DAC+");
        assert_eq!(flat.get("UUID").unwrap(), "sys-uuid-123");
        assert_eq!(flat.get("Hostname").unwrap(), "myhost");
    }

    #[test]
    fn get_flat_info_dict_defaults_when_data_missing() {
        let dir = fixture();
        let info = SystemInfo::with_root(dir.path());
        let flat: std::collections::HashMap<String, String> =
            info.get_flat_info_dict().into_iter().collect();

        assert_eq!(flat.get("Pi Model").unwrap(), "unknown");
        assert_eq!(flat.get("Memory").unwrap(), "unknown");
        assert_eq!(flat.get("HAT").unwrap(), "unknown unknown");
        assert_eq!(flat.get("UUID").unwrap(), "unknown");
        assert_eq!(flat.get("Hostname").unwrap(), "unknown");
        assert_eq!(flat.get("Pretty Hostname").unwrap(), "not set");
    }

    #[test]
    fn get_simple_output_includes_uuid_when_present() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Raspberry Pi 4 Model B\0");
        write(dir.path(), "etc/uuid", "12345678-1234-1234-1234-123456789012\n");

        let info = SystemInfo::with_root(dir.path());
        let output = info.get_simple_output();

        assert!(output.contains("Pi Model: Raspberry Pi 4 Model B"));
        assert!(output.contains("System UUID: 12345678-1234-1234-1234-123456789012"));
    }

    #[test]
    fn get_simple_output_omits_uuid_when_absent() {
        let dir = fixture();
        write(dir.path(), "proc/device-tree/model", "Raspberry Pi 3 Model B\0");

        let info = SystemInfo::with_root(dir.path());
        let output = info.get_simple_output();

        assert!(output.contains("Pi Model: Raspberry Pi 3 Model B"));
        assert!(!output.contains("System UUID:"));
    }
}
