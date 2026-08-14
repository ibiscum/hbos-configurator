//! Port of `configurator/volume.py` (headphone volume subset used by the API/CLI).
use std::collections::HashMap;

/// Known headphone volume control names, checked in priority order.
pub const HEADPHONE_VOLUME_CONTROLS: &[&str] = &["Headphone"];

pub const HEADPHONE_VOLUME_DB_KEY: &str = "system.volume.headphone";
pub const HEADPHONE_VOLUME_CARD_DB_KEY: &str = "system.volume.headphone.card";
pub const HEADPHONE_VOLUME_CONTROL_DB_KEY: &str = "system.volume.headphone.control";

/// Key/value configuration storage abstraction (mirrors `configurator.configdb.ConfigDB`).
///
/// A real sqlite-backed implementation has not been ported yet; callers can
/// substitute any implementation (e.g. an in-memory store for tests).
pub trait ConfigStore {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: &str, value: &str);
}

/// Simple in-memory `ConfigStore`, used as the default until ConfigDB is ported.
#[derive(Debug, Default, Clone)]
pub struct MemoryConfigStore(HashMap<String, String>);

impl ConfigStore for MemoryConfigStore {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_string(), value.to_string());
    }
}

/// Low-level ALSA mixer access (mirrors the `alsaaudio`/`amixer` calls in volume.py).
pub trait VolumeBackend {
    fn current_volume(&self, card_index: i32, control_name: &str) -> Option<String>;
    fn set_volume(&self, card_index: i32, control_name: &str, volume_value: &str) -> bool;
    fn list_controls(&self, card_index: Option<i32>) -> Vec<String>;
}

/// Real ALSA-backed implementation of [`VolumeBackend`].
pub struct AlsaVolumeBackend;

impl VolumeBackend for AlsaVolumeBackend {
    fn current_volume(&self, card_index: i32, control_name: &str) -> Option<String> {
        let mixer = alsa::Mixer::new(&format!("hw:{}", card_index), false).ok()?;
        let sid = alsa::mixer::SelemId::new(control_name, 0);
        let selem = mixer.find_selem(&sid)?;
        if !selem.has_playback_volume() {
            return None;
        }
        let (min, max) = selem.get_playback_volume_range();
        let vol = selem
            .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
            .ok()?;
        if max <= min {
            return None;
        }
        let pct = ((vol - min) as f64 / (max - min) as f64) * 100.0;
        Some(format!("{}", pct.round() as i32))
    }

    fn set_volume(&self, card_index: i32, control_name: &str, volume_value: &str) -> bool {
        let volume_int = match volume_value.parse::<f64>() {
            Ok(v) => (v as i32).clamp(0, 100),
            Err(_) => return false,
        };
        let mixer = match alsa::Mixer::new(&format!("hw:{}", card_index), false) {
            Ok(m) => m,
            Err(_) => return false,
        };
        let sid = alsa::mixer::SelemId::new(control_name, 0);
        let selem = match mixer.find_selem(&sid) {
            Some(s) => s,
            None => return false,
        };
        if !selem.has_playback_volume() {
            return false;
        }
        let (min, max) = selem.get_playback_volume_range();
        let raw = min + ((max - min) as f64 * volume_int as f64 / 100.0).round() as i64;
        selem.set_playback_volume_all(raw).is_ok()
    }

    fn list_controls(&self, card_index: Option<i32>) -> Vec<String> {
        let device = card_index
            .map(|c| format!("hw:{}", c))
            .unwrap_or_else(|| "default".to_string());
        let mixer = match alsa::Mixer::new(&device, false) {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        mixer
            .iter()
            .filter_map(alsa::mixer::Selem::new)
            .filter_map(|selem| selem.get_id().get_name().ok().map(|s| s.to_string()))
            .collect()
    }
}

/// Filter the sound card's available controls down to known headphone controls.
pub fn get_available_headphone_controls(
    backend: &dyn VolumeBackend,
    card_index: Option<i32>,
) -> Vec<String> {
    let Some(card_index) = card_index else {
        return Vec::new();
    };
    let available = backend.list_controls(Some(card_index));
    HEADPHONE_VOLUME_CONTROLS
        .iter()
        .filter(|c| available.iter().any(|a| a == *c))
        .map(|c| c.to_string())
        .collect()
}

/// Get the current headphone volume, returning `(volume, control_name)`.
pub fn get_headphone_volume(
    backend: &dyn VolumeBackend,
    card_index: Option<i32>,
) -> (Option<String>, Option<String>) {
    let Some(control) = get_available_headphone_controls(backend, card_index)
        .into_iter()
        .next()
    else {
        return (None, None);
    };
    let Some(card_index) = card_index else {
        return (None, None);
    };
    match backend.current_volume(card_index, &control) {
        Some(volume) => (Some(volume), Some(control)),
        None => (None, None),
    }
}

/// Set the headphone volume on the first available headphone control.
pub fn set_headphone_volume(
    backend: &dyn VolumeBackend,
    card_index: Option<i32>,
    volume_value: &str,
) -> bool {
    let Some(card_index) = card_index else {
        return false;
    };
    match get_available_headphone_controls(backend, Some(card_index))
        .into_iter()
        .next()
    {
        Some(control) => backend.set_volume(card_index, &control, volume_value),
        None => false,
    }
}

/// Store the current headphone volume setting into the configuration database.
pub fn store_headphone_volume(
    backend: &dyn VolumeBackend,
    store: &mut dyn ConfigStore,
    card_index: Option<i32>,
) -> bool {
    let Some(card_index) = card_index else {
        return false;
    };
    let Some(control) = get_available_headphone_controls(backend, Some(card_index))
        .into_iter()
        .next()
    else {
        return false;
    };
    match backend.current_volume(card_index, &control) {
        Some(volume) => {
            store.set(HEADPHONE_VOLUME_DB_KEY, &volume);
            store.set(HEADPHONE_VOLUME_CARD_DB_KEY, &card_index.to_string());
            store.set(HEADPHONE_VOLUME_CONTROL_DB_KEY, &control);
            true
        }
        None => false,
    }
}

/// Restore the headphone volume from the configuration database.
pub fn restore_headphone_volume(
    backend: &dyn VolumeBackend,
    store: &dyn ConfigStore,
    card_index: Option<i32>,
) -> bool {
    let Some(volume) = store.get(HEADPHONE_VOLUME_DB_KEY) else {
        return false;
    };
    let Some(card_index) = card_index else {
        return false;
    };
    match get_available_headphone_controls(backend, Some(card_index))
        .into_iter()
        .next()
    {
        Some(control) => backend.set_volume(card_index, &control, &volume),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeBackend {
        controls: Vec<String>,
        volumes: RefCell<HashMap<(i32, String), String>>,
        set_calls: RefCell<Vec<(i32, String, String)>>,
        fail_set: bool,
    }

    impl VolumeBackend for FakeBackend {
        fn current_volume(&self, card_index: i32, control_name: &str) -> Option<String> {
            self.volumes
                .borrow()
                .get(&(card_index, control_name.to_string()))
                .cloned()
        }

        fn set_volume(&self, card_index: i32, control_name: &str, volume_value: &str) -> bool {
            if self.fail_set {
                return false;
            }
            self.set_calls.borrow_mut().push((
                card_index,
                control_name.to_string(),
                volume_value.to_string(),
            ));
            self.volumes
                .borrow_mut()
                .insert((card_index, control_name.to_string()), volume_value.to_string());
            true
        }

        fn list_controls(&self, _card_index: Option<i32>) -> Vec<String> {
            self.controls.clone()
        }
    }

    #[test]
    fn get_available_headphone_controls_filters_known_names() {
        let backend = FakeBackend {
            controls: vec!["Master".to_string(), "Headphone".to_string()],
            ..Default::default()
        };
        assert_eq!(
            get_available_headphone_controls(&backend, Some(0)),
            vec!["Headphone".to_string()]
        );
    }

    #[test]
    fn get_available_headphone_controls_empty_without_card() {
        let backend = FakeBackend::default();
        assert_eq!(
            get_available_headphone_controls(&backend, None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn get_available_headphone_controls_empty_when_not_present() {
        let backend = FakeBackend {
            controls: vec!["Master".to_string()],
            ..Default::default()
        };
        assert!(get_available_headphone_controls(&backend, Some(0)).is_empty());
    }

    #[test]
    fn get_headphone_volume_returns_value_and_control() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        backend.set_volume(0, "Headphone", "75");
        assert_eq!(
            get_headphone_volume(&backend, Some(0)),
            (Some("75".to_string()), Some("Headphone".to_string()))
        );
    }

    #[test]
    fn get_headphone_volume_none_when_no_controls() {
        let backend = FakeBackend::default();
        assert_eq!(get_headphone_volume(&backend, Some(0)), (None, None));
    }

    #[test]
    fn set_headphone_volume_uses_first_control() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        assert!(set_headphone_volume(&backend, Some(0), "42"));
        assert_eq!(
            backend.set_calls.borrow().as_slice(),
            &[(0, "Headphone".to_string(), "42".to_string())]
        );
    }

    #[test]
    fn set_headphone_volume_fails_without_controls() {
        let backend = FakeBackend::default();
        assert!(!set_headphone_volume(&backend, Some(0), "42"));
    }

    #[test]
    fn set_headphone_volume_fails_without_card() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        assert!(!set_headphone_volume(&backend, None, "42"));
    }

    #[test]
    fn store_and_restore_headphone_volume_round_trip() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        backend.set_volume(0, "Headphone", "60");
        let mut store = MemoryConfigStore::default();

        assert!(store_headphone_volume(&backend, &mut store, Some(0)));
        assert_eq!(store.get(HEADPHONE_VOLUME_DB_KEY), Some("60".to_string()));
        assert_eq!(
            store.get(HEADPHONE_VOLUME_CARD_DB_KEY),
            Some("0".to_string())
        );
        assert_eq!(
            store.get(HEADPHONE_VOLUME_CONTROL_DB_KEY),
            Some("Headphone".to_string())
        );

        // Simulate the volume having changed, then restoring the stored value.
        backend.set_volume(0, "Headphone", "10");
        assert!(restore_headphone_volume(&backend, &store, Some(0)));
        assert_eq!(
            backend.current_volume(0, "Headphone"),
            Some("60".to_string())
        );
    }

    #[test]
    fn store_headphone_volume_fails_without_card() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        let mut store = MemoryConfigStore::default();
        assert!(!store_headphone_volume(&backend, &mut store, None));
    }

    #[test]
    fn restore_headphone_volume_fails_when_nothing_stored() {
        let backend = FakeBackend {
            controls: vec!["Headphone".to_string()],
            ..Default::default()
        };
        let store = MemoryConfigStore::default();
        assert!(!restore_headphone_volume(&backend, &store, Some(0)));
    }

    #[test]
    fn restore_headphone_volume_fails_without_controls() {
        let backend = FakeBackend::default();
        let mut store = MemoryConfigStore::default();
        store.set(HEADPHONE_VOLUME_DB_KEY, "50");
        assert!(!restore_headphone_volume(&backend, &store, Some(0)));
    }

    #[test]
    fn memory_config_store_round_trip() {
        let mut store = MemoryConfigStore::default();
        assert_eq!(store.get("missing"), None);
        store.set("k", "v");
        assert_eq!(store.get("k"), Some("v".to_string()));
    }
}
