//! Port of `configurator/pimodel.py` (Raspberry Pi model/version detection).
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Normalize a device-tree model name: trim whitespace and drop NUL bytes.
pub fn normalize_model_name(raw: &str) -> String {
    raw.trim().replace('\0', "")
}

/// Map a normalized model name to its short version token.
pub fn detect_version(model_name: &str) -> String {
    let version = if model_name.contains("3 Model B+") || model_name.contains("3 Model B Plus") {
        "3B+"
    } else if model_name.contains("3 Model A Plus") {
        "3A+"
    } else if model_name.contains("3 Model B") {
        "3B"
    } else if model_name.contains("4 Model B") {
        "4"
    } else if model_name.contains("Compute Module 4") {
        "CM4"
    } else if model_name.contains("Pi Zero W") {
        "0W"
    } else if model_name.contains("Pi Zero 2") {
        "0W2"
    } else if model_name.contains("Pi 2 Model") {
        "2"
    } else if model_name.contains("Pi 5 Model") {
        "5"
    } else if model_name.contains("Compute Module 5") {
        "CM5"
    } else {
        "unknown"
    };
    version.to_string()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PiModel {
    pub model_name: String,
    pub version: String,
}

impl PiModel {
    /// Detect the Raspberry Pi model by reading `/proc/device-tree/model`.
    pub fn new() -> Self {
        Self::with_root("/")
    }

    /// Detect the Raspberry Pi model, reading the device tree under `root`
    /// (injectable so tests can point at a fixture directory).
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        match fs::read_to_string(path_for(root.as_ref())) {
            Ok(content) => {
                let model_name = normalize_model_name(&content);
                let version = detect_version(&model_name);
                Self { model_name, version }
            }
            Err(_) => Self { model_name: "unknown".to_string(), version: "unknown".to_string() },
        }
    }

    pub fn get_model_name(&self) -> &str {
        &self.model_name
    }

    pub fn get_version(&self) -> &str {
        &self.version
    }
}

impl Default for PiModel {
    fn default() -> Self {
        Self::new()
    }
}

fn path_for(root: &Path) -> PathBuf {
    root.join("proc/device-tree/model")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_model(dir: &Path, content: &str) {
        let path = path_for(dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn normalize_model_name_removes_nulls_and_whitespace() {
        assert_eq!(normalize_model_name("Raspberry Pi 4 Model B Rev 1.4\x00\n"), "Raspberry Pi 4 Model B Rev 1.4");
    }

    #[test]
    fn detect_sets_model_and_version_with_null_byte_input() {
        let dir = fixture();
        write_model(dir.path(), "Raspberry Pi 4 Model B Rev 1.5\x00\n");
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_model_name(), "Raspberry Pi 4 Model B Rev 1.5");
        assert_eq!(model.get_version(), "4");
    }

    #[test]
    fn detect_file_not_found_keeps_unknowns() {
        let dir = fixture();
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_model_name(), "unknown");
        assert_eq!(model.get_version(), "unknown");
    }

    #[test]
    fn zero2_mapping() {
        let dir = fixture();
        write_model(dir.path(), "Raspberry Pi Zero 2 W Rev 1.0\x00\n");
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_version(), "0W2");
    }

    #[test]
    fn cm5_mapping_distinct_from_pi5() {
        let dir = fixture();
        write_model(dir.path(), "Raspberry Pi Compute Module 5\x00\n");
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_version(), "CM5");
    }

    #[test]
    fn pi5_mapping() {
        let dir = fixture();
        write_model(dir.path(), "Raspberry Pi 5 Model B Rev 1.0\x00\n");
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_version(), "5");
    }

    #[test]
    fn unknown_model_maps_to_unknown_version() {
        let dir = fixture();
        write_model(dir.path(), "Unknown Raspberry Pi Variant\x00\n");
        let model = PiModel::with_root(dir.path());
        assert_eq!(model.get_version(), "unknown");
    }
}
