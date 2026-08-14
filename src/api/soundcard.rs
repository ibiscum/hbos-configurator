//! Port of `configurator/soundcard.py` (HiFiBerry sound card catalogue and detection).
//!
//! Hardware access is abstracted behind [`CommandRunner`] (reused from
//! `soundcard_detector`) so detection and mixer-control management can be
//! exercised in tests the same way Python patches `subprocess.run`. The
//! native `alsaaudio`/`alsactl` bindings from the Python original are not
//! available here; hardware-index lookup and mixer-control checks fall back
//! to parsing `aplay -l`/`amixer` output, which the Python code also uses as
//! its own fallback path.
use serde::Serialize;

use crate::api::soundcard_detector::{
    detect_from_config_txt_comment, map_aplay_to_overlay, CommandRunner,
};

pub const UNKNOWN_CARD_NAME: &str = "Unknown";

/// ALSA state file template for creating a dummy software mixer control.
pub const ALSA_STATE_FILE_TEMPLATE: &str = r#"
state.sndrpihifiberry {
    control.98 {
        iface MIXER
        name '%CONTROL_NAME%'
        value.0 255
        value.1 255
        comment {
            access 'read write user'
            type INTEGER
            count 2
            range '0 - 255'
            tlv '0000000100000008ffffdcc400000023'
            dbmin -9020
            dbmax -95
            dbvalue.0 -95
            dbvalue.1 -95
        }
    }
}
"#;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct CardDefinition {
    pub aplay_contains: Option<&'static str>,
    pub arecord_contains: Option<&'static str>,
    pub hat_name: Option<&'static str>,
    pub volume_control: Option<&'static str>,
    pub headphone_volume_control: Option<&'static str>,
    pub output_channels: i32,
    pub input_channels: i32,
    pub features: &'static [&'static str],
    pub supports_dsp: bool,
    pub card_type: &'static [&'static str],
    pub dtoverlay: Option<&'static str>,
    pub is_pro: bool,
    pub aliases: &'static [&'static str],
}

const NONE_DEF: CardDefinition = CardDefinition {
    aplay_contains: None,
    arecord_contains: None,
    hat_name: None,
    volume_control: None,
    headphone_volume_control: None,
    output_channels: 2,
    input_channels: 0,
    features: &[],
    supports_dsp: false,
    card_type: &[],
    dtoverlay: None,
    is_pro: false,
    aliases: &[],
};

/// Sound card catalogue, ordered as in the Python `SOUND_CARD_DEFINITIONS` dict.
pub const SOUND_CARD_DEFINITIONS: &[(&str, CardDefinition)] = &[
    (
        "DAC8x/ADC8x",
        CardDefinition {
            aplay_contains: Some("DAC8xADC8x"),
            hat_name: Some("DAC8x"),
            output_channels: 8,
            input_channels: 8,
            card_type: &["DAC", "ADC"],
            dtoverlay: Some("hifiberry-dac8x"),
            ..NONE_DEF
        },
    ),
    (
        "DAC8x",
        CardDefinition {
            aplay_contains: Some("DAC8x"),
            hat_name: Some("DAC8x"),
            output_channels: 8,
            input_channels: 0,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dac8x"),
            ..NONE_DEF
        },
    ),
    (
        "Digi2 Pro",
        CardDefinition {
            hat_name: Some("Digi2 Pro"),
            output_channels: 2,
            input_channels: 0,
            features: &["dsp"],
            supports_dsp: true,
            card_type: &["Digi"],
            dtoverlay: Some("hifiberry-digi-pro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "Amp100",
        CardDefinition {
            hat_name: Some("Amp100"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 0,
            features: &["spdifnoclock", "toslink"],
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-amp100,automute"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "Amp3",
        CardDefinition {
            aplay_contains: Some("Amp3"),
            hat_name: Some("Amp3"),
            volume_control: Some("A.Mstr Vol"),
            output_channels: 2,
            input_channels: 0,
            features: &["usehwvolume"],
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-amp3"),
            ..NONE_DEF
        },
    ),
    (
        "Amp4",
        CardDefinition {
            hat_name: Some("Amp4"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 0,
            features: &["usehwvolume"],
            supports_dsp: true,
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-dacplus-std"),
            ..NONE_DEF
        },
    ),
    (
        "Amp4 Pro",
        CardDefinition {
            aplay_contains: Some("Amp4 Pro"),
            hat_name: Some("Amp4 Pro"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 0,
            features: &["usehwvolume"],
            supports_dsp: true,
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-amp4pro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "DAC+ ADC Pro",
        CardDefinition {
            aplay_contains: Some("DAC+ADC Pro"),
            hat_name: Some("DAC+ ADC Pro"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 2,
            features: &["analoginput"],
            card_type: &["DAC", "ADC"],
            dtoverlay: Some("hifiberry-dacplusadcpro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "DAC+ ADC",
        CardDefinition {
            aplay_contains: Some("DAC+ ADC"),
            hat_name: Some("DAC+ ADC"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 2,
            features: &["analoginput"],
            card_type: &["DAC", "ADC"],
            dtoverlay: Some("hifiberry-dacplusadc"),
            ..NONE_DEF
        },
    ),
    (
        "DAC2 ADC Pro",
        CardDefinition {
            aplay_contains: Some("DAC2 ADC Pro"),
            hat_name: Some("DAC2 ADC Pro"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 2,
            features: &["analoginput"],
            supports_dsp: true,
            card_type: &["DAC", "ADC"],
            dtoverlay: Some("hifiberry-dacplusadcpro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "DAC2 HD",
        CardDefinition {
            aplay_contains: Some("DAC+ HD"),
            hat_name: Some("DAC 2 HD"),
            volume_control: Some("DAC"),
            output_channels: 2,
            input_channels: 0,
            supports_dsp: true,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dacplushd"),
            is_pro: true,
            aliases: &["DAC2 HD", "DAC 2 HD", " DAC2HD"],
            ..NONE_DEF
        },
    ),
    (
        "DAC+ DSP",
        CardDefinition {
            aplay_contains: Some("DAC+DSP"),
            hat_name: Some("DAC+ DSP"),
            volume_control: Some("DSPVolume"),
            output_channels: 2,
            input_channels: 0,
            features: &["toslink"],
            supports_dsp: true,
            card_type: &["DAC", "Digi"],
            dtoverlay: Some("hifiberry-dacplusdsp"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "DAC+/Amp2",
        CardDefinition {
            aplay_contains: Some("DAC+"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dacplus-std"),
            aliases: &["DAC+", "Amp2"],
            ..NONE_DEF
        },
    ),
    (
        "DAC+ Pro",
        CardDefinition {
            aplay_contains: Some("DAC+ Pro"),
            hat_name: Some("DAC+ Pro"),
            volume_control: Some("Digital"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dacplus-pro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "DAC2 Pro",
        CardDefinition {
            hat_name: Some("DAC2 Pro"),
            volume_control: Some("Digital"),
            headphone_volume_control: Some("Headphone"),
            output_channels: 2,
            input_channels: 0,
            supports_dsp: true,
            card_type: &["DAC", "Headphone"],
            dtoverlay: Some("hifiberry-dacplus-pro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "Amp+",
        CardDefinition {
            aplay_contains: Some("AMP"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-amp"),
            ..NONE_DEF
        },
    ),
    (
        "Digi+ Pro",
        CardDefinition {
            aplay_contains: Some("Digi Pro"),
            output_channels: 2,
            input_channels: 0,
            features: &["digi"],
            supports_dsp: true,
            card_type: &["Digi"],
            dtoverlay: Some("hifiberry-digi-pro"),
            is_pro: true,
            ..NONE_DEF
        },
    ),
    (
        "Digi+",
        CardDefinition {
            aplay_contains: Some("Digi"),
            output_channels: 2,
            input_channels: 0,
            features: &["digi"],
            card_type: &["Digi"],
            dtoverlay: Some("hifiberry-digi"),
            ..NONE_DEF
        },
    ),
    (
        "Beocreate 4-Channel Amplifier",
        CardDefinition {
            aplay_contains: Some("beocreate"),
            hat_name: Some("Beocreate 4-Channel Amplifier"),
            volume_control: Some("DSPVolume"),
            output_channels: 2,
            input_channels: 0,
            features: &["dsp", "toslink"],
            supports_dsp: true,
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-dac"),
            is_pro: true,
            aliases: &["Beocreate 4CA"],
            ..NONE_DEF
        },
    ),
    (
        "DAC+ Light",
        CardDefinition {
            aplay_contains: Some("snd_rpi_hifiberry_dac"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dac"),
            ..NONE_DEF
        },
    ),
    (
        "DAC+ Zero",
        CardDefinition {
            aplay_contains: Some("snd_rpi_hifiberry_dac"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["DAC"],
            dtoverlay: Some("hifiberry-dac"),
            ..NONE_DEF
        },
    ),
    (
        "MiniAmp",
        CardDefinition {
            aplay_contains: Some("snd_rpi_hifiberry_dac"),
            output_channels: 2,
            input_channels: 0,
            card_type: &["Amp"],
            dtoverlay: Some("hifiberry-dac"),
            ..NONE_DEF
        },
    ),
    (
        "ADC",
        CardDefinition {
            arecord_contains: Some("snd_rpi_hifiberry_adc"),
            hat_name: Some("ADC"),
            output_channels: 0,
            input_channels: 2,
            card_type: &["ADC"],
            dtoverlay: Some("hifiberry-adc"),
            ..NONE_DEF
        },
    ),
    (
        "HDMI Audio",
        CardDefinition {
            output_channels: 2,
            input_channels: 0,
            card_type: &["Digi"],
            aliases: &["HDMI"],
            ..NONE_DEF
        },
    ),
    (
        "Null",
        CardDefinition {
            output_channels: 2,
            input_channels: 0,
            card_type: &["Null"],
            ..NONE_DEF
        },
    ),
];

/// Look up a card definition by its exact catalogue name.
pub fn find_by_name(name: &str) -> Option<(&'static str, CardDefinition)> {
    SOUND_CARD_DEFINITIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(n, d)| (*n, *d))
}

/// Look up a card definition by one of its aliases.
pub fn find_by_alias(name: &str) -> Option<(&'static str, CardDefinition)> {
    SOUND_CARD_DEFINITIONS
        .iter()
        .find(|(_, d)| d.aliases.contains(&name))
        .map(|(n, d)| (*n, *d))
}

/// Look up a card definition by its HAT EEPROM product name.
pub fn find_by_hat_name(hat_name: &str) -> Option<(&'static str, CardDefinition)> {
    SOUND_CARD_DEFINITIONS
        .iter()
        .find(|(_, d)| d.hat_name == Some(hat_name))
        .map(|(n, d)| (*n, *d))
}

/// Resolve a detected name (possibly `"CardA/CardB"`) against the catalogue:
/// exact match, then alias match, then (for multi-card overlays) exact/alias
/// match of each `/`-separated part.
pub fn resolve_card_definition(full_name: &str) -> Option<(&'static str, CardDefinition)> {
    if let Some(hit) = find_by_name(full_name) {
        return Some(hit);
    }
    if let Some(hit) = find_by_alias(full_name) {
        return Some(hit);
    }

    let parts: Vec<&str> = full_name.split('/').map(|s| s.trim()).collect();
    if parts.len() > 1 {
        for part in &parts {
            if let Some(hit) = find_by_name(part) {
                return Some(hit);
            }
        }
        for part in &parts {
            if let Some(hit) = find_by_alias(part) {
                return Some(hit);
            }
        }
    }
    None
}

fn overlay_base(overlay: &str) -> &str {
    overlay.split(',').next().unwrap_or(overlay)
}

/// Card names whose `dtoverlay` matches `overlay`, optionally restricted to
/// cards without a `hat_name` (mirrors `_overlay_to_card_name`'s `no_hat_only`).
pub fn cards_for_overlay(overlay: &str, no_hat_only: bool) -> Vec<&'static str> {
    let base = overlay_base(overlay);
    let mut all = Vec::new();
    let mut no_hat = Vec::new();
    for (name, def) in SOUND_CARD_DEFINITIONS {
        let Some(dtoverlay) = def.dtoverlay else {
            continue;
        };
        let dt_base = overlay_base(dtoverlay.strip_prefix("hifiberry-").unwrap_or(dtoverlay));
        if dt_base == base {
            all.push(*name);
            if def.hat_name.is_none() {
                no_hat.push(*name);
            }
        }
    }
    if no_hat_only && !no_hat.is_empty() {
        no_hat
    } else {
        all
    }
}

/// Card name (or `"/"`-joined names) for an overlay, preferring an explicit
/// HAT product name if given.
pub fn card_name_for_overlay(overlay: &str, hat_product: Option<&str>, no_hat_only: bool) -> Option<String> {
    if let Some(p) = hat_product {
        if !p.trim().is_empty() {
            return Some(p.to_string());
        }
    }
    let cards = cards_for_overlay(overlay, no_hat_only);
    if cards.is_empty() {
        None
    } else {
        Some(cards.join("/"))
    }
}

/// List all sound card definitions as formatted text (`"table"` or `"csv"`).
pub fn list_all_sound_cards(output_format: &str) -> String {
    let mut out = String::new();
    if output_format == "csv" {
        out.push_str("Name,DT Overlay,Volume Control,Output Channels,Input Channels,Features,Supports DSP,Card Type\n");
        for (name, def) in SOUND_CARD_DEFINITIONS {
            let dtoverlay = def.dtoverlay.unwrap_or("unknown");
            let volume_control = def.volume_control.unwrap_or("");
            let features = def.features.join(";");
            let card_types = def.card_type.join(";");
            let supports_dsp = if def.supports_dsp { "Yes" } else { "No" };
            out.push_str(&format!(
                "\"{name}\",\"{dtoverlay}\",\"{volume_control}\",{},{},\"{features}\",\"{supports_dsp}\",\"{card_types}\"\n",
                def.output_channels, def.input_channels
            ));
        }
    } else {
        out.push_str("Available HiFiBerry Sound Cards:\n");
        out.push_str(&"=".repeat(70));
        out.push('\n');
        out.push_str(&format!("{:<30} {:<30}\n", "Sound Card Name", "Device Tree Overlay"));
        out.push_str(&"-".repeat(70));
        out.push('\n');
        for (name, def) in SOUND_CARD_DEFINITIONS {
            let dtoverlay = def.dtoverlay.unwrap_or("unknown");
            out.push_str(&format!("{name:<30} {dtoverlay:<30}\n"));
        }
        out.push_str(&"-".repeat(70));
        out.push('\n');
        out.push_str(&format!("Total: {} sound cards\n", SOUND_CARD_DEFINITIONS.len()));
    }
    out
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Soundcard {
    pub name: String,
    pub volume_control: Option<String>,
    pub headphone_volume_control: Option<String>,
    pub output_channels: i32,
    pub input_channels: i32,
    pub features: Vec<String>,
    pub hat_name: Option<String>,
    pub supports_dsp: bool,
    pub card_type: Vec<String>,
}

impl Soundcard {
    pub fn unknown() -> Self {
        Self {
            name: UNKNOWN_CARD_NAME.to_string(),
            volume_control: None,
            headphone_volume_control: None,
            output_channels: 2,
            input_channels: 0,
            features: Vec::new(),
            hat_name: None,
            supports_dsp: false,
            card_type: Vec::new(),
        }
    }

    pub fn from_definition(name: &str, def: &CardDefinition) -> Self {
        Self {
            name: name.to_string(),
            volume_control: def.volume_control.map(|s| s.to_string()),
            headphone_volume_control: def.headphone_volume_control.map(|s| s.to_string()),
            output_channels: def.output_channels,
            input_channels: def.input_channels,
            features: def.features.iter().map(|s| s.to_string()).collect(),
            hat_name: def.hat_name.map(|s| s.to_string()),
            supports_dsp: def.supports_dsp,
            card_type: def.card_type.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Detect the connected sound card.
    ///
    /// `config_lines` are the lines of config.txt (for the pinned-card
    /// comment bypass) and `hat_product` is the HAT EEPROM product name, if
    /// available. When `no_eeprom` is `true`, detection relies solely on
    /// `aplay -l` (mirrors Python's `_detect_card_aplay_only`).
    pub fn detect(
        no_eeprom: bool,
        config_lines: &[String],
        hat_product: Option<&str>,
        runner: &dyn CommandRunner,
    ) -> Self {
        if no_eeprom {
            return Self::detect_aplay_only(runner);
        }

        let aplay_output = runner.run(&["aplay", "-l"]).unwrap_or_default();
        let has_hifiberry_aplay = aplay_output.to_lowercase().contains("hifiberry");

        if let Some(comment_name) = detect_from_config_txt_comment(config_lines) {
            if has_hifiberry_aplay {
                if let Some((name, def)) = find_by_name(&comment_name) {
                    return Self::from_definition(name, &def);
                }
            }
        }

        if has_hifiberry_aplay {
            if let Some(line) = aplay_output
                .lines()
                .find(|l| l.to_lowercase().contains("hifiberry") && l.contains('[') && l.contains(']'))
            {
                if let Some(overlay) = map_aplay_to_overlay(line) {
                    let has_hat = hat_product.map(|p| !p.trim().is_empty()).unwrap_or(false);
                    if let Some(card_name) = card_name_for_overlay(&overlay, hat_product, !has_hat) {
                        if let Some(hit) = resolve_card_definition(&card_name) {
                            let (name, def) = additional_card_checks(&aplay_output, hit, runner);
                            return Self::from_definition(name, &def);
                        }
                    }
                }
            }
        }

        if let Some(p) = hat_product {
            if !p.trim().is_empty() {
                if let Some((name, def)) = find_by_hat_name(p).or_else(|| resolve_card_definition(p)) {
                    return Self::from_definition(name, &def);
                }
            }
        }

        Self::unknown()
    }

    fn detect_aplay_only(runner: &dyn CommandRunner) -> Self {
        let output = runner.run(&["aplay", "-l"]).unwrap_or_default();
        if !output.to_lowercase().contains("hifiberry") {
            return Self::unknown();
        }
        for line in output.lines() {
            let lower = line.to_lowercase();
            if !lower.contains("hifiberry") || !line.contains('[') || !line.contains(']') {
                continue;
            }
            let Some(overlay) = map_aplay_to_overlay(line) else {
                continue;
            };
            let mut candidates = cards_for_overlay(&overlay, true);
            if candidates.is_empty() {
                candidates = cards_for_overlay(&overlay, false);
            }
            if let Some(&name) = candidates.first() {
                if let Some((name, def)) = find_by_name(name) {
                    let (name, def) = additional_card_checks(&output, (name, def), runner);
                    return Self::from_definition(name, &def);
                }
            }
        }
        Self::unknown()
    }

    /// Returns the mixer control name, falling back to `"Softvol"` if none
    /// is defined and `use_softvol_fallback` is `true`.
    pub fn get_mixer_control_name(&self, use_softvol_fallback: bool) -> Option<String> {
        self.volume_control
            .clone()
            .or_else(|| use_softvol_fallback.then(|| "Softvol".to_string()))
    }

    pub fn get_headphone_volume_control_name(&self) -> Option<String> {
        self.headphone_volume_control.clone()
    }

    /// Hardware card index, parsed from `aplay -l` (`"card N: ..."`).
    pub fn get_hardware_index(&self, runner: &dyn CommandRunner) -> Option<i32> {
        let output = runner.run(&["aplay", "-l"])?;
        for line in output.lines() {
            if !line.to_lowercase().contains("hifiberry") {
                continue;
            }
            let card_part = line.split(':').next()?.trim();
            if let Some(rest) = card_part.strip_prefix("card ") {
                if let Ok(index) = rest.trim().parse::<i32>() {
                    return Some(index);
                }
            }
        }
        None
    }

    /// Whether `control_name` shows up in `amixer -c <index>` output.
    pub fn check_mixer_control_exists(&self, control_name: &str, runner: &dyn CommandRunner) -> bool {
        let Some(index) = self.get_hardware_index(runner) else {
            return false;
        };
        runner
            .run(&["amixer", "-c", &index.to_string()])
            .map(|output| output.contains(control_name))
            .unwrap_or(false)
    }

    /// Create a dummy software ALSA mixer control via `alsactl restore`.
    pub fn create_dummy_alsa_control(&self, control_name: &str, runner: &dyn CommandRunner) -> bool {
        if self.check_mixer_control_exists(control_name, runner) {
            return true;
        }

        let content = ALSA_STATE_FILE_TEMPLATE.replace("%CONTROL_NAME%", control_name);
        let Ok(tmp) = write_temp_state_file(&content) else {
            return false;
        };
        let path = tmp.to_string_lossy().to_string();
        runner.run(&["/usr/sbin/alsactl", "-f", &path, "restore"]);
        let _ = std::fs::remove_file(&tmp);

        self.check_mixer_control_exists(control_name, runner)
    }

    /// Get the existing volume control, or create a `"Softvol"` (or
    /// `preferred_name`) dummy one if none exists.
    pub fn get_or_create_volume_control(
        &self,
        preferred_name: Option<&str>,
        runner: &dyn CommandRunner,
    ) -> Option<String> {
        if let Some(vc) = &self.volume_control {
            return Some(vc.clone());
        }
        let control_name = preferred_name.unwrap_or("Softvol");
        if self.create_dummy_alsa_control(control_name, runner) {
            Some(control_name.to_string())
        } else {
            None
        }
    }
}

fn write_temp_state_file(content: &str) -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!("hbos-alsa-{}-{:?}.state", std::process::id(), std::thread::current().id()));
    std::fs::write(&path, content)?;
    Ok(path)
}

/// Refine `initial` when aplay/HAT detection lands on a card that shares its
/// overlay with another (DAC+ Pro vs. DAC2 Pro, distinguished by whether the
/// card exposes a "Headphone" mixer control).
fn additional_card_checks(
    aplay_output: &str,
    initial: (&'static str, CardDefinition),
    runner: &dyn CommandRunner,
) -> (&'static str, CardDefinition) {
    let (name, _) = initial;
    if matches!(name, "DAC+ Pro" | "DAC2 Pro" | "DAC+/Amp2") && aplay_output.to_lowercase().contains("dacplus") {
        return distinguish_dac_pro_models(aplay_output, initial, runner);
    }
    initial
}

fn distinguish_dac_pro_models(
    aplay_output: &str,
    initial: (&'static str, CardDefinition),
    runner: &dyn CommandRunner,
) -> (&'static str, CardDefinition) {
    if !aplay_output.contains("HiFiBerry DAC+ Pro") {
        return initial;
    }

    let card_number = aplay_output.lines().find_map(|line| {
        let lower = line.to_lowercase();
        if lower.contains("hifiberry") && lower.contains("dacplus") && line.trim_start().starts_with("card ") {
            line.split(':').next()?.split_whitespace().nth(1)?.parse::<i32>().ok()
        } else {
            None
        }
    });

    let Some(number) = card_number else {
        return initial;
    };

    let amixer_output = runner.run(&["amixer", "-c", &number.to_string()]).unwrap_or_default();
    let target = if amixer_output.to_lowercase().contains("headphone") {
        "DAC2 Pro"
    } else {
        "DAC+ Pro"
    };
    find_by_name(target).unwrap_or(initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubRunner {
        aplay: Option<String>,
        amixer: Option<String>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, args: &[&str]) -> Option<String> {
            match args.first() {
                Some(&"aplay") => self.aplay.clone(),
                Some(&"amixer") => self.amixer.clone(),
                _ => None,
            }
        }
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_by_exact_name() {
        let (name, def) = find_by_name("DAC+ DSP").unwrap();
        assert_eq!(name, "DAC+ DSP");
        assert_eq!(def.volume_control, Some("DSPVolume"));
    }

    #[test]
    fn finds_by_alias() {
        let (name, _) = find_by_alias("Beocreate 4CA").unwrap();
        assert_eq!(name, "Beocreate 4-Channel Amplifier");
    }

    #[test]
    fn resolve_prefers_exact_match_over_alias() {
        let (name, _) = resolve_card_definition("DAC2 HD").unwrap();
        assert_eq!(name, "DAC2 HD");
    }

    #[test]
    fn resolve_splits_multi_card_overlay_names() {
        let (name, _) = resolve_card_definition("Unknown Thing/DAC+ DSP").unwrap();
        assert_eq!(name, "DAC+ DSP");
    }

    #[test]
    fn resolve_unknown_name_returns_none() {
        assert!(resolve_card_definition("Not A Real Card").is_none());
    }

    #[test]
    fn cards_for_overlay_filters_hat_only_cards() {
        let cards = cards_for_overlay("dacplus-std", true);
        assert_eq!(cards, vec!["DAC+/Amp2"]);
    }

    #[test]
    fn cards_for_overlay_returns_all_when_no_hat_only_false() {
        let cards = cards_for_overlay("dacplus-std", false);
        assert!(cards.contains(&"Amp4"));
        assert!(cards.contains(&"DAC+/Amp2"));
    }

    #[test]
    fn card_name_for_overlay_prefers_hat_product() {
        let name = card_name_for_overlay("dacplusdsp", Some("DAC+ DSP"), false);
        assert_eq!(name, Some("DAC+ DSP".to_string()));
    }

    #[test]
    fn card_name_for_overlay_falls_back_to_catalogue() {
        let name = card_name_for_overlay("digi", None, false);
        assert_eq!(name, Some("Digi+".to_string()));
    }

    #[test]
    fn list_all_sound_cards_table_contains_header_and_entries() {
        let output = list_all_sound_cards("table");
        assert!(output.contains("Available HiFiBerry Sound Cards:"));
        assert!(output.contains("DAC+ DSP"));
        assert!(output.contains(&format!("Total: {} sound cards", SOUND_CARD_DEFINITIONS.len())));
    }

    #[test]
    fn list_all_sound_cards_csv_contains_header_and_entries() {
        let output = list_all_sound_cards("csv");
        assert!(output.starts_with("Name,DT Overlay"));
        assert!(output.contains("\"DAC+ DSP\""));
    }

    #[test]
    fn detect_prefers_config_txt_comment_when_aplay_confirms_hifiberry() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dacplusdsp]".to_string()),
            amixer: None,
        };
        let card = Soundcard::detect(
            false,
            &lines(&["# HiFiBerry card: DAC+ DSP"]),
            None,
            &runner,
        );
        assert_eq!(card.name, "DAC+ DSP");
        assert_eq!(card.volume_control, Some("DSPVolume".to_string()));
    }

    #[test]
    fn detect_ignores_config_txt_comment_when_aplay_shows_no_hifiberry_card() {
        let runner = StubRunner {
            aplay: Some("card 0: bcm2835 [bcm2835 Headphones]".to_string()),
            amixer: None,
        };
        let card = Soundcard::detect(false, &lines(&["# HiFiBerry card: DAC+ DSP"]), None, &runner);
        assert_eq!(card.name, UNKNOWN_CARD_NAME);
    }

    #[test]
    fn detect_uses_aplay_as_highest_priority_source() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_amp3]".to_string()),
            amixer: None,
        };
        let card = Soundcard::detect(false, &lines(&[]), None, &runner);
        assert_eq!(card.name, "Amp3");
    }

    #[test]
    fn detect_falls_back_to_hat_product_when_aplay_has_no_hifiberry_card() {
        let runner = StubRunner {
            aplay: Some("card 0: bcm2835 [bcm2835 Headphones]".to_string()),
            amixer: None,
        };
        let card = Soundcard::detect(false, &lines(&[]), Some("DAC+ DSP"), &runner);
        assert_eq!(card.name, "DAC+ DSP");
    }

    #[test]
    fn detect_returns_unknown_when_nothing_matches() {
        let runner = StubRunner { aplay: None, amixer: None };
        let card = Soundcard::detect(false, &lines(&[]), None, &runner);
        assert_eq!(card.name, UNKNOWN_CARD_NAME);
    }

    #[test]
    fn detect_no_eeprom_uses_aplay_only() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_digi]".to_string()),
            amixer: None,
        };
        let card = Soundcard::detect(true, &lines(&[]), Some("Amp100"), &runner);
        assert_eq!(card.name, "Digi+");
    }

    #[test]
    fn detect_distinguishes_dac2_pro_via_headphone_mixer_control() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dacplus HiFiBerry DAC+ Pro]".to_string()),
            amixer: Some("Simple mixer control 'Headphone',0".to_string()),
        };
        let card = Soundcard::detect(false, &lines(&[]), None, &runner);
        assert_eq!(card.name, "DAC2 Pro");
    }

    #[test]
    fn detect_keeps_dac_plus_pro_without_headphone_mixer_control() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dacplus HiFiBerry DAC+ Pro]".to_string()),
            amixer: Some("Simple mixer control 'Digital',0".to_string()),
        };
        let card = Soundcard::detect(false, &lines(&[]), None, &runner);
        assert_eq!(card.name, "DAC+ Pro");
    }

    #[test]
    fn get_mixer_control_name_returns_defined_control() {
        let card = Soundcard::from_definition("DAC+ DSP", &find_by_name("DAC+ DSP").unwrap().1);
        assert_eq!(card.get_mixer_control_name(false), Some("DSPVolume".to_string()));
    }

    #[test]
    fn get_mixer_control_name_falls_back_to_softvol() {
        let card = Soundcard::unknown();
        assert_eq!(card.get_mixer_control_name(true), Some("Softvol".to_string()));
        assert_eq!(card.get_mixer_control_name(false), None);
    }

    #[test]
    fn get_headphone_volume_control_name_present() {
        let card = Soundcard::from_definition("DAC2 Pro", &find_by_name("DAC2 Pro").unwrap().1);
        assert_eq!(card.get_headphone_volume_control_name(), Some("Headphone".to_string()));
    }

    #[test]
    fn get_hardware_index_parses_aplay_output() {
        let runner = StubRunner {
            aplay: Some("card 2: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
            amixer: None,
        };
        let card = Soundcard::unknown();
        assert_eq!(card.get_hardware_index(&runner), Some(2));
    }

    #[test]
    fn get_hardware_index_none_when_no_hifiberry_card() {
        let runner = StubRunner {
            aplay: Some("card 0: bcm2835 [bcm2835 Headphones]".to_string()),
            amixer: None,
        };
        let card = Soundcard::unknown();
        assert_eq!(card.get_hardware_index(&runner), None);
    }

    #[test]
    fn check_mixer_control_exists_true_when_present_in_amixer_output() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
            amixer: Some("Simple mixer control 'Softvol',0".to_string()),
        };
        let card = Soundcard::unknown();
        assert!(card.check_mixer_control_exists("Softvol", &runner));
        assert!(!card.check_mixer_control_exists("Missing", &runner));
    }

    #[test]
    fn create_dummy_alsa_control_returns_true_when_already_exists() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
            amixer: Some("Simple mixer control 'Softvol',0".to_string()),
        };
        let card = Soundcard::unknown();
        assert!(card.create_dummy_alsa_control("Softvol", &runner));
    }

    #[test]
    fn create_dummy_alsa_control_returns_false_when_still_missing_after_restore() {
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
            amixer: Some("Simple mixer control 'Other',0".to_string()),
        };
        let card = Soundcard::unknown();
        assert!(!card.create_dummy_alsa_control("Softvol", &runner));
    }

    #[test]
    fn get_or_create_volume_control_returns_existing_control() {
        let card = Soundcard::from_definition("DAC+ DSP", &find_by_name("DAC+ DSP").unwrap().1);
        let runner = StubRunner { aplay: None, amixer: None };
        assert_eq!(
            card.get_or_create_volume_control(None, &runner),
            Some("DSPVolume".to_string())
        );
    }

    #[test]
    fn get_or_create_volume_control_creates_softvol_when_missing() {
        let card = Soundcard::unknown();
        let runner = StubRunner {
            aplay: Some("card 0: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
            amixer: Some("Simple mixer control 'Softvol',0".to_string()),
        };
        assert_eq!(
            card.get_or_create_volume_control(None, &runner),
            Some("Softvol".to_string())
        );
    }
}
