//! Port of `configurator/soundcard_detector.py` (HiFiBerry sound card auto-detection).
//!
//! This is a scoped port: the Python original also consults a config
//! database, retries HAT EEPROM reads, probes I2C addresses directly and
//! refines DAC+DSP vs. Beocreate via a live DSP firmware checksum fetched
//! from `sigmatcpserver`. Those paths depend on modules (`configdb`,
//! `hattools`, `dsptoolkit`, the `soundcard` card-definition table) that
//! have not been ported into this crate yet, so they are omitted here.
//! What is ported faithfully: the config.txt HiFiBerry-comment bypass,
//! `aplay -l` / `arecord -l` output mapping, the HAT-product-name overlay
//! map, and config.txt overlay/comment read-modify-write logic.
use std::path::PathBuf;
use std::process::Command;

/// Marker comment prefix identifying a pinned HiFiBerry card in config.txt.
pub const HIFIBERRY_CARD_COMMENT_PREFIX: &str = "# HiFiBerry card:";

/// Runs external commands. `None` mirrors Python's `_run_command` catching
/// `CalledProcessError`/`FileNotFoundError` and returning `""`.
pub trait CommandRunner {
    fn run(&self, args: &[&str]) -> Option<String>;
}

/// Real implementation that spawns processes via [`std::process::Command`].
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, args: &[&str]) -> Option<String> {
        let (program, rest) = args.split_first()?;
        match Command::new(program).args(rest).output() {
            Ok(out) if out.status.success() => {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            _ => None,
        }
    }
}

/// Parse config.txt lines and return the card name from a HiFiBerry comment,
/// e.g. `# HiFiBerry card: DAC+ DSP` -> `Some("DAC+ DSP")`.
pub fn detect_from_config_txt_comment(lines: &[String]) -> Option<String> {
    for line in lines {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix(HIFIBERRY_CARD_COMMENT_PREFIX) {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Remove existing HiFiBerry card comment lines from config.txt, returning
/// the number of lines removed.
pub fn remove_hifiberry_comments(lines: &mut Vec<String>) -> usize {
    let before = lines.len();
    lines.retain(|line| !line.trim().starts_with(HIFIBERRY_CARD_COMMENT_PREFIX));
    before - lines.len()
}

/// Insert a `# HiFiBerry card: <name>` comment directly before the
/// `dtoverlay=<overlay_name>` line. Returns `false` if the overlay line
/// wasn't found.
pub fn add_card_comment_before_overlay(
    lines: &mut Vec<String>,
    overlay_name: &str,
    card_name: &str,
) -> bool {
    let overlay_line = format!("dtoverlay={overlay_name}");
    if let Some(pos) = lines.iter().position(|line| line.trim() == overlay_line) {
        lines.insert(pos, format!("{HIFIBERRY_CARD_COMMENT_PREFIX} {card_name}"));
        true
    } else {
        false
    }
}

/// Overlay names for HiFiBerry cards currently configured in config.txt.
pub fn current_hifiberry_overlays(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let stripped = line.trim();
            stripped
                .strip_prefix("dtoverlay=hifiberry")
                .map(|_| stripped.strip_prefix("dtoverlay=").unwrap().to_string())
        })
        .collect()
}

/// Map `aplay -l` output to a HiFiBerry overlay name. Patterns are checked
/// most-specific-first, then a generic keyword fallback is applied.
pub fn map_aplay_to_overlay(aplay_output: &str) -> Option<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("snd_rpi_hifiberry_dacplusadcpro", "dacplusadcpro"),
        ("snd_rpi_hifiberrydacplusdsp", "dacplusdsp"),
        ("snd_rpi_hifiberry_dacplusadc", "dacplusadc"),
        ("snd_rpi_hifiberry_dacplushd", "dacplushd"),
        ("snd_rpi_hifiberry_dacplus", "dacplus-std"),
        ("snd_rpi_hifiberry_amp4pro", "amp4pro"),
        ("snd_rpi_hifiberry_amp100", "amp100"),
        ("snd_rpi_hifiberry_amp3", "amp3"),
        ("snd_rpi_hifiberry_dac8x", "dac8x"),
        ("snd_rpi_hifiberry_amp", "amp"),
        ("snd_rpi_hifiberry_digi", "digi"),
        ("snd_rpi_hifiberry_dac", "dac"),
        ("pcm5102a-hifi", "dac"),
    ];

    let lower = aplay_output.to_lowercase();
    for (pattern, overlay) in PATTERNS {
        if lower.contains(pattern) {
            return Some(overlay.to_string());
        }
    }

    if lower.contains("dacplusdsp") || lower.contains("dsp") {
        Some("dacplusdsp".to_string())
    } else if lower.contains("dacplusadcpro") {
        Some("dacplusadcpro".to_string())
    } else if lower.contains("dacplusadc") {
        Some("dacplusadc".to_string())
    } else if lower.contains("dacplus") {
        Some("dacplus-std".to_string())
    } else if lower.contains("digi") {
        Some("digi".to_string())
    } else if lower.contains("amp") {
        Some("amp".to_string())
    } else {
        None
    }
}

/// Map a HAT EEPROM product name to its overlay name.
pub fn map_hat_to_overlay(hat_card: &str) -> Option<String> {
    const CARD_MAP: &[(&str, &str)] = &[
        ("Amp100", "amp100,automute"),
        ("DAC+ ADC Pro", "dacplusadcpro"),
        ("DAC+ ADC", "dacplusadc"),
        ("DAC2 ADC Pro", "dacplusadcpro"),
        ("DAC2 Pro", "dacplus-pro"),
        ("DAC 2 HD", "dacplushd"),
        ("Digi2 Pro", "digi-pro"),
        ("Amp3", "amp3"),
        ("Amp4 Pro", "amp4pro"),
        ("Amp4", "dacplus-std"),
        ("DAC8x", "dac8x"),
        ("StudioDAC8x", "dac8x"),
        ("DAC+ DSP", "dacplusdsp"),
        ("Digi Pro", "digi-pro"),
    ];
    CARD_MAP
        .iter()
        .find(|(name, _)| *name == hat_card)
        .map(|(_, overlay)| overlay.to_string())
}

/// Return the first HiFiBerry `aplay -l` line, optionally filtering out
/// PCM5102-based cards.
pub fn find_hifiberry_card_from_aplay(output: &str, include_pcm5102: bool) -> Option<String> {
    for line in output.lines() {
        let lower = line.to_lowercase();
        if !lower.contains("hifiberry") {
            continue;
        }
        if !include_pcm5102 && lower.contains("pcm5102") {
            continue;
        }
        return Some(line.trim().to_string());
    }
    None
}

/// Whether `expected_overlay` (e.g. `hifiberry-dacplusdsp`) is already
/// configured among the current HiFiBerry overlay lines.
pub fn is_overlay_already_configured(lines: &[String], expected_overlay: &str) -> bool {
    current_hifiberry_overlays(lines)
        .iter()
        .any(|overlay| overlay == expected_overlay)
}

/// Detects and configures the connected HiFiBerry sound card.
pub struct SoundcardDetector {
    pub config_lines: Vec<String>,
    pub reboot_file: PathBuf,
    pub detected_card: Option<String>,
    pub detected_overlay: Option<String>,
    pub include_pcm5102: bool,
    runner: Box<dyn CommandRunner>,
}

impl SoundcardDetector {
    pub fn new(config_lines: Vec<String>, reboot_file: PathBuf) -> Self {
        Self::with_runner(config_lines, reboot_file, Box::new(SystemCommandRunner))
    }

    pub fn with_runner(
        config_lines: Vec<String>,
        reboot_file: PathBuf,
        runner: Box<dyn CommandRunner>,
    ) -> Self {
        Self {
            config_lines,
            reboot_file,
            detected_card: None,
            detected_overlay: None,
            include_pcm5102: false,
            runner,
        }
    }

    fn run_command(&self, args: &[&str]) -> String {
        self.runner.run(args).unwrap_or_default()
    }

    fn detect_from_aplay(&self) -> Option<String> {
        let output = self.run_command(&["aplay", "-l"]);
        if output.is_empty() {
            return None;
        }
        let found = find_hifiberry_card_from_aplay(&output, self.include_pcm5102)?;
        map_aplay_to_overlay(&found)
    }

    /// Input-only cards (e.g. the ADC) have no playback device and never
    /// appear in `aplay -l`; match `arecord -l` instead using the same
    /// keyword mapping.
    fn detect_from_arecord(&self) -> Option<String> {
        let output = self.run_command(&["arecord", "-l"]);
        if output.is_empty() {
            return None;
        }
        let found = find_hifiberry_card_from_aplay(&output, self.include_pcm5102)?;
        map_aplay_to_overlay(&found)
    }

    /// Detect the connected HiFiBerry sound card.
    ///
    /// `hat_product` is the HAT EEPROM product name, if already read by the
    /// caller (see `api::systeminfo::read_hat_info`); passing `None` skips
    /// the HAT-based detection step.
    ///
    /// Detection order: config.txt card comment -> HAT product name ->
    /// `aplay -l` -> `arecord -l`.
    pub fn detect_card(&mut self, hat_product: Option<&str>) -> Option<String> {
        if let Some(card) = detect_from_config_txt_comment(&self.config_lines) {
            self.detected_card = Some(card.clone());
            self.detected_overlay = None;
            return Some(card);
        }

        if let Some(hat_card) = hat_product {
            if let Some(overlay) = map_hat_to_overlay(hat_card) {
                self.detected_overlay = Some(overlay);
                self.detected_card = Some(hat_card.to_string());
                return self.detected_card.clone();
            }
        }

        if let Some(overlay) = self.detect_from_aplay() {
            self.detected_overlay = Some(overlay);
            self.detected_card = self.detected_overlay.clone();
            return self.detected_card.clone();
        }

        if let Some(overlay) = self.detect_from_arecord() {
            self.detected_overlay = Some(overlay);
            self.detected_card = self.detected_overlay.clone();
            return self.detected_card.clone();
        }

        self.detected_card = None;
        self.detected_overlay = None;
        None
    }

    /// Write the detected overlay/comment into config.txt. Returns `true`
    /// if config.txt content changed (a reboot marker file is written in
    /// that case), `false` if the overlay was already configured or nothing
    /// was detected.
    pub fn configure_card(&mut self) -> std::io::Result<bool> {
        let Some(overlay) = self.detected_overlay.clone() else {
            return Ok(false);
        };
        let card_name = self.detected_card.clone().unwrap_or_default();
        let expected_overlay = format!("hifiberry-{overlay}");

        if is_overlay_already_configured(&self.config_lines, &expected_overlay) {
            remove_hifiberry_comments(&mut self.config_lines);
            add_card_comment_before_overlay(&mut self.config_lines, &expected_overlay, &card_name);
            return Ok(false);
        }

        self.config_lines
            .retain(|line| !line.trim().starts_with("dtoverlay=hifiberry"));
        remove_hifiberry_comments(&mut self.config_lines);
        self.config_lines.push(format!("dtoverlay={expected_overlay}"));
        add_card_comment_before_overlay(&mut self.config_lines, &expected_overlay, &card_name);

        std::fs::write(
            &self.reboot_file,
            format!("Configuring {card_name} requires a reboot.\n"),
        )?;

        Ok(true)
    }

    /// Detect the card and optionally persist it to config.txt.
    pub fn detect_and_configure(
        &mut self,
        hat_product: Option<&str>,
        store: bool,
        fallback_dac: bool,
    ) -> std::io::Result<Option<String>> {
        self.detect_card(hat_product);

        if self.detected_card.is_none() && fallback_dac {
            self.detected_overlay = Some("dac".to_string());
            self.detected_card = Some("DAC+ Light".to_string());
        }

        if store {
            self.configure_card()?;
        }

        Ok(self.detected_card.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    struct StubRunner {
        aplay: Option<String>,
        arecord: Option<String>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, args: &[&str]) -> Option<String> {
            match args.first() {
                Some(&"aplay") => self.aplay.clone(),
                Some(&"arecord") => self.arecord.clone(),
                _ => None,
            }
        }
    }

    #[test]
    fn detects_card_from_config_txt_comment() {
        let lines = lines(&["dtparam=audio=on", "# HiFiBerry card: DAC+ DSP", "dtoverlay=hifiberry-dacplusdsp"]);
        assert_eq!(
            detect_from_config_txt_comment(&lines),
            Some("DAC+ DSP".to_string())
        );
    }

    #[test]
    fn config_txt_comment_missing_returns_none() {
        let lines = lines(&["dtparam=audio=on"]);
        assert_eq!(detect_from_config_txt_comment(&lines), None);
    }

    #[test]
    fn config_txt_comment_empty_name_is_ignored() {
        let lines = lines(&["# HiFiBerry card:   "]);
        assert_eq!(detect_from_config_txt_comment(&lines), None);
    }

    #[test]
    fn removes_hifiberry_comments() {
        let mut lines = lines(&["# HiFiBerry card: DAC+ DSP", "dtoverlay=hifiberry-dacplusdsp", "other"]);
        let removed = remove_hifiberry_comments(&mut lines);
        assert_eq!(removed, 1);
        assert_eq!(lines, vec!["dtoverlay=hifiberry-dacplusdsp".to_string(), "other".to_string()]);
    }

    #[test]
    fn adds_comment_before_overlay_line() {
        let mut lines = lines(&["dtparam=audio=on", "dtoverlay=hifiberry-dacplusdsp"]);
        assert!(add_card_comment_before_overlay(&mut lines, "hifiberry-dacplusdsp", "DAC+ DSP"));
        assert_eq!(
            lines,
            vec![
                "dtparam=audio=on".to_string(),
                "# HiFiBerry card: DAC+ DSP".to_string(),
                "dtoverlay=hifiberry-dacplusdsp".to_string(),
            ]
        );
    }

    #[test]
    fn add_comment_returns_false_when_overlay_missing() {
        let mut lines = lines(&["dtparam=audio=on"]);
        assert!(!add_card_comment_before_overlay(&mut lines, "hifiberry-dacplusdsp", "DAC+ DSP"));
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn current_hifiberry_overlays_lists_all() {
        let lines = lines(&["dtoverlay=hifiberry-dacplusdsp", "dtoverlay=vc4-kms-v3d", "dtoverlay=hifiberry-amp"]);
        assert_eq!(
            current_hifiberry_overlays(&lines),
            vec!["hifiberry-dacplusdsp".to_string(), "hifiberry-amp".to_string()]
        );
    }

    #[test]
    fn maps_aplay_output_to_overlay_specific_pattern() {
        assert_eq!(
            map_aplay_to_overlay("card 0: sndrpihifiberry [snd_rpi_hifiberry_dacplusadcpro], device 0"),
            Some("dacplusadcpro".to_string())
        );
        assert_eq!(
            map_aplay_to_overlay("snd_rpi_hifiberry_amp100"),
            Some("amp100".to_string())
        );
    }

    #[test]
    fn maps_aplay_output_generic_fallback() {
        assert_eq!(map_aplay_to_overlay("some dsp thing"), Some("dacplusdsp".to_string()));
        assert_eq!(map_aplay_to_overlay("hifiberry digi card"), Some("digi".to_string()));
    }

    #[test]
    fn maps_aplay_output_unknown_returns_none() {
        assert_eq!(map_aplay_to_overlay("bcm2835 headphones"), None);
    }

    #[test]
    fn maps_hat_product_to_overlay() {
        assert_eq!(map_hat_to_overlay("DAC+ DSP"), Some("dacplusdsp".to_string()));
        assert_eq!(map_hat_to_overlay("Amp100"), Some("amp100,automute".to_string()));
        assert_eq!(map_hat_to_overlay("Unknown Card"), None);
    }

    #[test]
    fn finds_hifiberry_card_from_aplay_output() {
        let output = "card 0: bcm2835 [bcm2835 Headphones]\ncard 1: sndrpihifiberry [snd_rpi_hifiberry_dac]";
        assert_eq!(
            find_hifiberry_card_from_aplay(output, false),
            Some("card 1: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string())
        );
    }

    #[test]
    fn finds_hifiberry_excludes_pcm5102_by_default() {
        let output = "card 1: sndrpihifiberry [snd_rpi_hifiberry pcm5102a-hifi]";
        assert_eq!(find_hifiberry_card_from_aplay(output, false), None);
        assert!(find_hifiberry_card_from_aplay(output, true).is_some());
    }

    #[test]
    fn no_hifiberry_card_returns_none() {
        assert_eq!(find_hifiberry_card_from_aplay("card 0: bcm2835", false), None);
    }

    #[test]
    fn overlay_already_configured_check() {
        let lines = lines(&["dtoverlay=hifiberry-dacplusdsp"]);
        assert!(is_overlay_already_configured(&lines, "hifiberry-dacplusdsp"));
        assert!(!is_overlay_already_configured(&lines, "hifiberry-amp"));
    }

    #[test]
    fn detect_card_prefers_config_txt_comment_over_hardware() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&["# HiFiBerry card: DAC+ DSP"]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        let card = detector.detect_card(Some("Amp100"));
        assert_eq!(card, Some("DAC+ DSP".to_string()));
        assert_eq!(detector.detected_overlay, None);
    }

    #[test]
    fn detect_card_uses_hat_product_when_no_comment() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        let card = detector.detect_card(Some("DAC+ DSP"));
        assert_eq!(card, Some("DAC+ DSP".to_string()));
        assert_eq!(detector.detected_overlay, Some("dacplusdsp".to_string()));
    }

    #[test]
    fn detect_card_falls_back_to_aplay() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner {
                aplay: Some("card 1: sndrpihifiberry [snd_rpi_hifiberry_amp]".to_string()),
                arecord: None,
            }),
        );
        let card = detector.detect_card(None);
        assert_eq!(card, Some("amp".to_string()));
        assert_eq!(detector.detected_overlay, Some("amp".to_string()));
    }

    #[test]
    fn detect_card_falls_back_to_arecord_for_input_only_cards() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner {
                aplay: None,
                arecord: Some("card 1: sndrpihifiberry [snd_rpi_hifiberry_dacplusadc]".to_string()),
            }),
        );
        let card = detector.detect_card(None);
        assert_eq!(card, Some("dacplusadc".to_string()));
    }

    #[test]
    fn detect_card_returns_none_when_nothing_found() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        assert_eq!(detector.detect_card(None), None);
        assert_eq!(detector.detected_card, None);
    }

    #[test]
    fn configure_card_adds_overlay_and_writes_reboot_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let reboot_file = tmp.path().join("reboot");
        let mut detector = SoundcardDetector::with_runner(
            lines(&["dtparam=audio=on"]),
            reboot_file.clone(),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        detector.detected_overlay = Some("dacplusdsp".to_string());
        detector.detected_card = Some("DAC+ DSP".to_string());

        let changed = detector.configure_card().unwrap();
        assert!(changed);
        assert!(detector
            .config_lines
            .contains(&"dtoverlay=hifiberry-dacplusdsp".to_string()));
        assert!(detector
            .config_lines
            .contains(&"# HiFiBerry card: DAC+ DSP".to_string()));
        assert!(reboot_file.exists());
    }

    #[test]
    fn configure_card_replaces_existing_overlay() {
        let tmp = tempfile::tempdir().unwrap();
        let reboot_file = tmp.path().join("reboot");
        let mut detector = SoundcardDetector::with_runner(
            lines(&["dtoverlay=hifiberry-amp", "# HiFiBerry card: Amp+"]),
            reboot_file,
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        detector.detected_overlay = Some("dacplusdsp".to_string());
        detector.detected_card = Some("DAC+ DSP".to_string());

        detector.configure_card().unwrap();
        assert!(!detector
            .config_lines
            .iter()
            .any(|l| l == "dtoverlay=hifiberry-amp"));
        assert!(detector
            .config_lines
            .contains(&"dtoverlay=hifiberry-dacplusdsp".to_string()));
    }

    #[test]
    fn configure_card_noop_when_already_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let reboot_file = tmp.path().join("reboot");
        let mut detector = SoundcardDetector::with_runner(
            lines(&["dtoverlay=hifiberry-dacplusdsp"]),
            reboot_file.clone(),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        detector.detected_overlay = Some("dacplusdsp".to_string());
        detector.detected_card = Some("DAC+ DSP".to_string());

        let changed = detector.configure_card().unwrap();
        assert!(!changed);
        assert!(!reboot_file.exists());
    }

    #[test]
    fn configure_card_returns_false_without_detected_overlay() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        assert!(!detector.configure_card().unwrap());
    }

    #[test]
    fn detect_and_configure_applies_fallback_dac_when_nothing_detected() {
        let mut detector = SoundcardDetector::with_runner(
            lines(&[]),
            PathBuf::from("/tmp/does-not-matter-reboot"),
            Box::new(StubRunner { aplay: None, arecord: None }),
        );
        let card = detector
            .detect_and_configure(None, false, true)
            .unwrap();
        assert_eq!(card, Some("DAC+ Light".to_string()));
        assert_eq!(detector.detected_overlay, Some("dac".to_string()));
    }

    #[test]
    fn detect_and_configure_stores_when_requested() {
        let tmp = tempfile::tempdir().unwrap();
        let reboot_file = tmp.path().join("reboot");
        let mut detector = SoundcardDetector::with_runner(
            lines(&["dtparam=audio=on"]),
            reboot_file.clone(),
            Box::new(StubRunner {
                aplay: Some("card 1: sndrpihifiberry [snd_rpi_hifiberry_dac]".to_string()),
                arecord: None,
            }),
        );
        detector.detect_and_configure(None, true, false).unwrap();
        assert!(reboot_file.exists());
    }
}
