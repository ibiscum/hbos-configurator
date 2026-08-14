//! Port of `configurator/hattools.py` (HAT EEPROM vendor/product/UUID retrieval).
//!
//! The Python original reads HAT EEPROM data via the external `hateeprom`
//! package (I2C EEPROM access). No such binding exists in this project;
//! instead (reusing `api::systeminfo::read_hat_info`, which this module's
//! Python original is itself paired with elsewhere in the app) the real
//! reader sources vendor/product/UUID from the kernel-parsed device tree at
//! `/proc/device-tree/hat/*`, which is the same data the EEPROM driver
//! exposes on a running Raspberry Pi.
use std::path::Path;

use serde::Serialize;

pub const DEFAULT_VENDOR: &str = "no vendor";
pub const DEFAULT_PRODUCT: &str = "no product";
pub const DEFAULT_UUID: &str = "unknown";

/// A raw HAT metadata field value, as returned by the (dynamically typed,
/// in Python) EEPROM reader. `Other` models any non-string value, which
/// Python's `_normalize_hat_field` always discards.
#[derive(Debug, Clone, PartialEq)]
pub enum HatFieldValue {
    Str(String),
    Other,
}

/// Raw response from a HAT EEPROM short-info read.
#[derive(Debug, Clone, Default)]
pub struct HatShortInfo {
    pub success: bool,
    pub vendor: Option<HatFieldValue>,
    pub product: Option<HatFieldValue>,
    pub uuid: Option<HatFieldValue>,
}

/// HAT EEPROM access abstraction (mirrors the Python code's `HatEEPROM`
/// class from the external `hateeprom` package).
pub trait HatEepromReader: Send + Sync {
    /// `None` mirrors `HatEEPROM is None` (the reader/hardware support
    /// isn't available at all).
    fn short_info(&self) -> Option<HatShortInfo>;
}

/// Real implementation sourcing HAT info from `/proc/device-tree/hat/*`.
pub struct SysfsHatEepromReader<'a> {
    pub root: &'a Path,
}

impl HatEepromReader for SysfsHatEepromReader<'_> {
    fn short_info(&self) -> Option<HatShortInfo> {
        let raw = crate::api::systeminfo::read_hat_info(self.root);
        let success = raw.vendor.is_some() || raw.product.is_some() || raw.uuid.is_some();
        Some(HatShortInfo {
            success,
            vendor: raw.vendor.map(HatFieldValue::Str),
            product: raw.product.map(HatFieldValue::Str),
            uuid: raw.uuid.map(HatFieldValue::Str),
        })
    }
}

fn normalize_hat_field(value: Option<&HatFieldValue>) -> Option<String> {
    match value {
        Some(HatFieldValue::Str(s)) if s != "Unknown" => Some(s.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HatInfo {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub uuid: Option<String>,
}

/// Return HAT vendor/product/UUID; unknown/missing values are `None`.
pub fn get_hat_info(reader: &dyn HatEepromReader, verbose: bool) -> HatInfo {
    let Some(info) = reader.short_info() else {
        if verbose {
            tracing::warn!("hateeprom module not available, returning default values");
        }
        return HatInfo::default();
    };

    if !info.success {
        if verbose {
            tracing::error!("HAT EEPROM read failed");
        }
        return HatInfo::default();
    }

    HatInfo { vendor: normalize_hat_field(info.vendor.as_ref()), product: normalize_hat_field(info.product.as_ref()), uuid: normalize_hat_field(info.uuid.as_ref()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubReader(Option<HatShortInfo>);

    impl HatEepromReader for StubReader {
        fn short_info(&self) -> Option<HatShortInfo> {
            self.0.clone()
        }
    }

    #[test]
    fn success_with_all_values() {
        let reader = StubReader(Some(HatShortInfo {
            success: true,
            vendor: Some(HatFieldValue::Str("HiFiBerry".to_string())),
            product: Some(HatFieldValue::Str("DAC+ Pro".to_string())),
            uuid: Some(HatFieldValue::Str("12345678-1234-5678-1234-567812345678".to_string())),
        }));
        let info = get_hat_info(&reader, false);
        assert_eq!(info.vendor, Some("HiFiBerry".to_string()));
        assert_eq!(info.product, Some("DAC+ Pro".to_string()));
        assert_eq!(info.uuid, Some("12345678-1234-5678-1234-567812345678".to_string()));
    }

    #[test]
    fn success_with_unknown_values_becomes_none() {
        let reader = StubReader(Some(HatShortInfo {
            success: true,
            vendor: Some(HatFieldValue::Str("Unknown".to_string())),
            product: Some(HatFieldValue::Str("Unknown".to_string())),
            uuid: Some(HatFieldValue::Str("Unknown".to_string())),
        }));
        let info = get_hat_info(&reader, false);
        assert_eq!(info, HatInfo::default());
    }

    #[test]
    fn success_with_partial_unknown_values() {
        let reader = StubReader(Some(HatShortInfo {
            success: true,
            vendor: Some(HatFieldValue::Str("HiFiBerry".to_string())),
            product: Some(HatFieldValue::Str("Unknown".to_string())),
            uuid: Some(HatFieldValue::Str("12345678".to_string())),
        }));
        let info = get_hat_info(&reader, false);
        assert_eq!(info.vendor, Some("HiFiBerry".to_string()));
        assert_eq!(info.product, None);
        assert_eq!(info.uuid, Some("12345678".to_string()));
    }

    #[test]
    fn failure_returns_all_none() {
        let reader = StubReader(Some(HatShortInfo { success: false, ..Default::default() }));
        assert_eq!(get_hat_info(&reader, false), HatInfo::default());
    }

    #[test]
    fn reader_unavailable_returns_all_none() {
        let reader = StubReader(None);
        assert_eq!(get_hat_info(&reader, false), HatInfo::default());
        // Verbose path must not panic even though nothing else is observable here.
        assert_eq!(get_hat_info(&reader, true), HatInfo::default());
    }

    #[test]
    fn missing_fields_become_none() {
        let reader = StubReader(Some(HatShortInfo { success: true, vendor: Some(HatFieldValue::Str("HiFiBerry".to_string())), product: None, uuid: None }));
        let info = get_hat_info(&reader, false);
        assert_eq!(info, HatInfo { vendor: Some("HiFiBerry".to_string()), product: None, uuid: None });
    }

    #[test]
    fn non_string_fields_become_none() {
        let reader = StubReader(Some(HatShortInfo { success: true, vendor: Some(HatFieldValue::Other), product: Some(HatFieldValue::Other), uuid: None }));
        assert_eq!(get_hat_info(&reader, false), HatInfo::default());
    }

    #[test]
    fn default_constants_match_python() {
        assert_eq!(DEFAULT_VENDOR, "no vendor");
        assert_eq!(DEFAULT_PRODUCT, "no product");
        assert_eq!(DEFAULT_UUID, "unknown");
    }

    #[test]
    fn sysfs_reader_reflects_device_tree_hat_files() {
        let dir = tempfile::tempdir().unwrap();
        let hat_dir = dir.path().join("proc/device-tree/hat");
        std::fs::create_dir_all(&hat_dir).unwrap();
        std::fs::write(hat_dir.join("vendor"), "HiFiBerry\0").unwrap();
        std::fs::write(hat_dir.join("product"), "DAC+ Pro\0").unwrap();

        let reader = SysfsHatEepromReader { root: dir.path() };
        let info = get_hat_info(&reader, false);
        assert_eq!(info.vendor, Some("HiFiBerry".to_string()));
        assert_eq!(info.product, Some("DAC+ Pro".to_string()));
        assert_eq!(info.uuid, None);
    }

    #[test]
    fn sysfs_reader_no_hat_present_returns_all_none() {
        let dir = tempfile::tempdir().unwrap();
        let reader = SysfsHatEepromReader { root: dir.path() };
        assert_eq!(get_hat_info(&reader, false), HatInfo::default());
    }
}
