//! Port of `configurator/settings_manager.py`: modules register save/restore
//! callbacks for settings that should persist across restarts. Values are
//! stored via a `ConfigDb` under keys prefixed with `"saved-setting."`.
use std::collections::HashMap;

pub const SETTING_PREFIX: &str = "saved-setting.";

/// Key/value configuration storage abstraction (mirrors `configurator.configdb.ConfigDB`).
pub trait ConfigDb: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: &str, value: &str);
    /// All entries whose key starts with `prefix` (or all entries if `None`).
    fn get_all(&self, prefix: Option<&str>) -> HashMap<String, String>;
    fn delete(&mut self, key: &str);
}

/// Simple in-memory `ConfigDb`, used as the default until a persistent
/// ConfigDB implementation is ported.
#[derive(Debug, Default)]
pub struct MemoryConfigDb {
    store: HashMap<String, String>,
    pub deleted: Vec<String>,
}

impl ConfigDb for MemoryConfigDb {
    fn get(&self, key: &str) -> Option<String> {
        self.store.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: &str) {
        self.store.insert(key.to_string(), value.to_string());
    }

    fn get_all(&self, prefix: Option<&str>) -> HashMap<String, String> {
        match prefix {
            Some(p) => self
                .store
                .iter()
                .filter(|(k, _)| k.starts_with(p))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            None => self.store.clone(),
        }
    }

    fn delete(&mut self, key: &str) {
        self.deleted.push(key.to_string());
        self.store.remove(key);
    }
}

/// Returns the current setting value to persist, serialized as a string, or
/// `None` to skip saving (mirrors Python's `SaveCallback`).
pub type SaveCallback = Box<dyn Fn() -> Option<String> + Send + Sync>;
/// Applies a previously stored string value, or returns `Err` on failure
/// (mirrors Python's `RestoreCallback` raising an exception).
pub type RestoreCallback = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

struct RegisteredSetting {
    save: SaveCallback,
    restore: RestoreCallback,
}

/// Manages saving and restoring of application settings via a [`ConfigDb`].
pub struct SettingsManager {
    configdb: Box<dyn ConfigDb>,
    // Vec (not HashMap) to preserve registration order, mirroring Python dict ordering.
    registered: Vec<(String, RegisteredSetting)>,
    setting_prefix: String,
}

impl SettingsManager {
    pub fn new(configdb: Box<dyn ConfigDb>) -> Self {
        Self {
            configdb,
            registered: Vec::new(),
            setting_prefix: SETTING_PREFIX.to_string(),
        }
    }

    fn key_for(&self, setting_name: &str) -> String {
        format!("{}{}", self.setting_prefix, setting_name)
    }

    fn find_index(&self, setting_name: &str) -> Option<usize> {
        self.registered.iter().position(|(name, _)| name == setting_name)
    }

    /// Register a setting with save/restore callbacks. Re-registering an
    /// existing name replaces its callbacks in place.
    pub fn register_setting(&mut self, setting_name: &str, save: SaveCallback, restore: RestoreCallback) {
        let entry = RegisteredSetting { save, restore };
        if let Some(idx) = self.find_index(setting_name) {
            tracing::warn!("Replacing existing registration for setting: {setting_name}");
            self.registered[idx].1 = entry;
        } else {
            self.registered.push((setting_name.to_string(), entry));
        }
    }

    /// Save a specific setting using its registered save callback.
    pub fn save_setting(&mut self, setting_name: &str) -> bool {
        let Some(idx) = self.find_index(setting_name) else {
            tracing::error!("Setting '{setting_name}' is not registered");
            return false;
        };
        let value = (self.registered[idx].1.save)();
        match value {
            Some(value) => {
                let key = self.key_for(setting_name);
                self.configdb.set(&key, &value);
                true
            }
            None => false,
        }
    }

    /// Restore a specific setting using its registered restore callback.
    pub fn restore_setting(&mut self, setting_name: &str) -> bool {
        let Some(idx) = self.find_index(setting_name) else {
            tracing::error!("Setting '{setting_name}' is not registered");
            return false;
        };
        let key = self.key_for(setting_name);
        match self.configdb.get(&key) {
            Some(value) => (self.registered[idx].1.restore)(&value).is_ok(),
            None => false,
        }
    }

    /// Save all registered settings, returning a name -> success map.
    pub fn save_all_settings(&mut self) -> HashMap<String, bool> {
        let names: Vec<String> = self.registered.iter().map(|(n, _)| n.clone()).collect();
        names.into_iter().map(|name| (name.clone(), self.save_setting(&name))).collect()
    }

    /// Restore all registered settings, returning a name -> success map.
    pub fn restore_all_settings(&mut self) -> HashMap<String, bool> {
        let names: Vec<String> = self.registered.iter().map(|(n, _)| n.clone()).collect();
        names.into_iter().map(|name| (name.clone(), self.restore_setting(&name))).collect()
    }

    /// Names of all registered settings, in registration order.
    pub fn list_registered_settings(&self) -> Vec<String> {
        self.registered.iter().map(|(n, _)| n.clone()).collect()
    }

    /// All saved settings currently stored in the config database.
    pub fn list_saved_settings(&self) -> HashMap<String, String> {
        self.configdb
            .get_all(Some(&self.setting_prefix))
            .into_iter()
            .filter_map(|(key, value)| {
                let suffix = key.strip_prefix(&self.setting_prefix)?;
                if suffix.is_empty() {
                    return None;
                }
                Some((suffix.to_string(), value))
            })
            .collect()
    }

    /// Delete a saved setting from the config database.
    pub fn delete_saved_setting(&mut self, setting_name: &str) -> bool {
        if setting_name.is_empty() {
            tracing::error!("Setting name must not be empty");
            return false;
        }
        let key = self.key_for(setting_name);
        self.configdb.delete(&key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn manager() -> SettingsManager {
        SettingsManager::new(Box::new(MemoryConfigDb::default()))
    }

    #[test]
    fn register_replaces_existing_callback() {
        let mut mgr = manager();
        mgr.register_setting("alpha", Box::new(|| Some("v1".to_string())), Box::new(|_v| Ok(())));
        mgr.register_setting("alpha", Box::new(|| Some("v2".to_string())), Box::new(|_v| Ok(())));

        assert_eq!(mgr.list_registered_settings(), vec!["alpha".to_string()]);
        assert!(mgr.save_setting("alpha"));
        assert_eq!(mgr.list_saved_settings().get("alpha"), Some(&"v2".to_string()));
    }

    #[test]
    fn save_setting_serializes_non_string_value() {
        let mut mgr = manager();
        mgr.register_setting("num", Box::new(|| Some(123.to_string())), Box::new(|_v| Ok(())));

        assert!(mgr.save_setting("num"));
        assert_eq!(mgr.list_saved_settings().get("num"), Some(&"123".to_string()));
    }

    #[test]
    fn save_setting_none_value_returns_false() {
        let mut mgr = manager();
        mgr.register_setting("empty", Box::new(|| None), Box::new(|_v| Ok(())));

        assert!(!mgr.save_setting("empty"));
        assert!(!mgr.list_saved_settings().contains_key("empty"));
    }

    #[test]
    fn restore_setting_passes_string_to_callback() {
        let mut mgr = manager();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_clone = observed.clone();

        mgr.register_setting(
            "mode",
            Box::new(|| Some("x".to_string())),
            Box::new(move |v| {
                observed_clone.lock().unwrap().push(v.to_string());
                Ok(())
            }),
        );
        // Seed a stored value directly, mirroring `self.db.store["saved-setting.mode"] = 7`.
        assert!(mgr.save_setting("mode"));
        mgr.registered_configdb_set("mode", "7");

        assert!(mgr.restore_setting("mode"));
        assert_eq!(*observed.lock().unwrap(), vec!["7".to_string()]);
    }

    #[test]
    fn restore_setting_callback_exception_returns_false() {
        let mut mgr = manager();
        mgr.register_setting(
            "bad",
            Box::new(|| Some("ok".to_string())),
            Box::new(|_v| Err("boom".to_string())),
        );
        mgr.registered_configdb_set("bad", "value");

        assert!(!mgr.restore_setting("bad"));
    }

    #[test]
    fn unregistered_setting_returns_false() {
        let mut mgr = manager();
        assert!(!mgr.save_setting("missing"));
        assert!(!mgr.restore_setting("missing"));
    }

    #[test]
    fn list_saved_settings_skips_empty_suffix() {
        let mut mgr = manager();
        mgr.registered_configdb_set("", "oops");
        mgr.registered_configdb_set("good", "value");

        let saved = mgr.list_saved_settings();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved.get("good"), Some(&"value".to_string()));
    }

    #[test]
    fn delete_saved_setting_rejects_empty_name() {
        let mut mgr = manager();
        assert!(!mgr.delete_saved_setting(""));
    }

    #[test]
    fn delete_saved_setting_success() {
        let mut mgr = manager();
        assert!(mgr.delete_saved_setting("alpha"));
    }

    #[test]
    fn save_all_and_restore_all_report_per_setting_results() {
        let mut mgr = manager();
        mgr.register_setting("a", Box::new(|| Some("1".to_string())), Box::new(|_v| Ok(())));
        mgr.register_setting("b", Box::new(|| None), Box::new(|_v| Ok(())));

        let save_results = mgr.save_all_settings();
        assert_eq!(save_results.get("a"), Some(&true));
        assert_eq!(save_results.get("b"), Some(&false));

        let restore_results = mgr.restore_all_settings();
        assert_eq!(restore_results.get("a"), Some(&true));
        assert_eq!(restore_results.get("b"), Some(&false));
    }

    impl SettingsManager {
        /// Test helper: write directly into the backing config db with the
        /// `saved-setting.` prefix applied, bypassing save callbacks.
        fn registered_configdb_set(&mut self, setting_name: &str, value: &str) {
            let key = self.key_for(setting_name);
            self.configdb.set(&key, value);
        }
    }
}
