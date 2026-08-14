//! CLI wrapper mirroring `configurator.soundcard:main` (the `config-soundcard` tool).
use std::fs;

use clap::{Parser, ValueEnum};

use crate::api::soundcard::{self, Soundcard, UNKNOWN_CARD_NAME};
use crate::api::soundcard_detector::SystemCommandRunner;
use crate::api::systeminfo::read_hat_info;

const CONFIG_TXT_PATH: &str = "/boot/firmware/config.txt";

#[derive(ValueEnum, Clone, Debug, PartialEq, Default)]
pub enum ListFormat {
    #[default]
    Table,
    Csv,
}

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-soundcard", about = "Detect and display sound card details")]
pub struct SoundcardArgs {
    /// Enable verbose logging (INFO level)
    #[arg(short, long)]
    pub verbose: bool,

    /// Enable very verbose logging (DEBUG level)
    #[arg(long = "very-verbose")]
    pub very_verbose: bool,

    /// List all available HiFiBerry sound cards with their device tree overlays
    #[arg(long)]
    pub list: bool,

    /// Output format for --list
    #[arg(long = "list-format", value_enum, default_value_t = ListFormat::Table)]
    pub list_format: ListFormat,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Print only the name of the detected sound card
    #[arg(long)]
    pub name: bool,

    /// Print only the volume control of the detected sound card
    #[arg(long = "volume-control")]
    pub volume_control: bool,

    /// Print the volume control, falling back to "Softvol" if none defined
    #[arg(long = "volume-control-softvol")]
    pub volume_control_softvol: bool,

    /// Print only the headphone volume control of the detected sound card
    #[arg(long = "headphone-volume-control")]
    pub headphone_volume_control: bool,

    /// Print only the hardware index of the detected sound card
    #[arg(long)]
    pub hw: bool,

    /// Print only the number of output channels
    #[arg(long = "output-channels")]
    pub output_channels: bool,

    /// Print only the number of input channels
    #[arg(long = "input-channels")]
    pub input_channels: bool,

    /// Print only the features of the detected sound card
    #[arg(long)]
    pub features: bool,

    /// Exit 0 if the detected card has input channels (an ADC), exit 1 otherwise
    #[arg(long = "has-input")]
    pub has_input: bool,

    /// Disable EEPROM check and use only aplay -l for detection
    #[arg(long = "no-eeprom")]
    pub no_eeprom: bool,

    /// Create a dummy ALSA volume control with the specified name
    #[arg(long = "create-volume-control", value_name = "CONTROL_NAME")]
    pub create_volume_control: Option<String>,

    /// Get existing volume control or create a dummy one (defaults to "Softvol")
    #[arg(long = "get-or-create-volume-control", value_name = "CONTROL_NAME")]
    pub get_or_create_volume_control: Option<String>,

    /// Print the name of the detected sound card if one is found, nothing otherwise
    #[arg(long)]
    pub detected: bool,
}

fn read_config_lines(path: &str) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| content.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

fn detect(no_eeprom: bool) -> Soundcard {
    let config_lines = read_config_lines(CONFIG_TXT_PATH);
    let hat = read_hat_info(std::path::Path::new("/"));
    let runner = SystemCommandRunner;
    Soundcard::detect(no_eeprom, &config_lines, hat.product.as_deref(), &runner)
}

/// Run the soundcard CLI. Returns the process exit code (mirrors Python's `sys.exit`).
pub fn run(args: &SoundcardArgs) -> i32 {
    if args.list {
        let format = match args.list_format {
            ListFormat::Table => "table",
            ListFormat::Csv => "csv",
        };
        print!("{}", soundcard::list_all_sound_cards(format));
        return 0;
    }

    if args.detected {
        let card = detect(args.no_eeprom);
        if card.name != UNKNOWN_CARD_NAME {
            println!("{}", card.name);
            return 0;
        }
        return 1;
    }

    if args.has_input {
        let card = detect(args.no_eeprom);
        return if card.input_channels > 0 { 0 } else { 1 };
    }

    let runner = SystemCommandRunner;

    if let Some(control_name) = &args.create_volume_control {
        let card = detect(args.no_eeprom);
        if card.create_dummy_alsa_control(control_name, &runner) {
            println!("Successfully created volume control: {control_name}");
            return 0;
        }
        println!("Failed to create volume control: {control_name}");
        return 1;
    }

    if let Some(preferred) = &args.get_or_create_volume_control {
        let card = detect(args.no_eeprom);
        match card.get_or_create_volume_control(Some(preferred), &runner) {
            Some(control_name) => {
                println!("{control_name}");
                0
            }
            None => {
                println!("Failed to get or create volume control");
                1
            }
        }
    } else {
        let card = detect(args.no_eeprom);

        if args.json {
            let data = serde_json::json!({
                "name": card.name,
                "volume_control": card.volume_control,
                "headphone_volume_control": card.headphone_volume_control,
                "hardware_index": card.get_hardware_index(&runner),
                "output_channels": card.output_channels,
                "input_channels": card.input_channels,
                "features": card.features,
                "hat_name": card.hat_name,
                "supports_dsp": card.supports_dsp,
                "card_type": card.card_type,
            });
            println!("{}", serde_json::to_string_pretty(&data).unwrap());
        } else if args.name {
            println!("{}", card.name);
        } else if args.volume_control {
            println!("{}", card.volume_control.clone().unwrap_or_default());
        } else if args.volume_control_softvol {
            println!("{}", card.get_mixer_control_name(true).unwrap());
        } else if args.headphone_volume_control {
            println!("{}", card.get_headphone_volume_control_name().unwrap_or_default());
        } else if args.hw {
            match card.get_hardware_index(&runner) {
                Some(index) => println!("{index}"),
                None => println!(),
            }
        } else if args.output_channels {
            println!("{}", card.output_channels);
        } else if args.input_channels {
            println!("{}", card.input_channels);
        } else if args.features {
            println!("{}", card.features.join(","));
        } else {
            println!("Sound card details:");
            println!("Name: {}", card.name);
            println!("Volume Control: {}", card.volume_control.clone().unwrap_or_default());
            println!(
                "Headphone Volume Control: {}",
                card.headphone_volume_control.clone().unwrap_or_else(|| "None".to_string())
            );
            match card.get_hardware_index(&runner) {
                Some(index) => println!("Hardware Index: {index}"),
                None => println!("Hardware Index: "),
            }
            println!("Output Channels: {}", card.output_channels);
            println!("Input Channels: {}", card.input_channels);
            println!(
                "Features: {}",
                if card.features.is_empty() { "None".to_string() } else { card.features.join(",") }
            );
            println!("HAT Name: {}", card.hat_name.clone().unwrap_or_else(|| "None".to_string()));
            println!("Supports DSP: {}", if card.supports_dsp { "Yes" } else { "No" });
            println!(
                "Card Type: {}",
                if card.card_type.is_empty() { "None".to_string() } else { card.card_type.join(", ") }
            );
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = SoundcardArgs::parse_from(["config-soundcard"]);
        assert!(!args.verbose);
        assert!(!args.list);
        assert_eq!(args.list_format, ListFormat::Table);
        assert!(!args.json);
    }

    #[test]
    fn parses_list_with_csv_format() {
        let args = SoundcardArgs::parse_from(["config-soundcard", "--list", "--list-format", "csv"]);
        assert!(args.list);
        assert_eq!(args.list_format, ListFormat::Csv);
    }

    #[test]
    fn parses_detection_flags() {
        let args = SoundcardArgs::parse_from(["config-soundcard", "--detected", "--no-eeprom"]);
        assert!(args.detected);
        assert!(args.no_eeprom);
    }

    #[test]
    fn parses_create_volume_control_value() {
        let args = SoundcardArgs::parse_from(["config-soundcard", "--create-volume-control", "Softvol"]);
        assert_eq!(args.create_volume_control, Some("Softvol".to_string()));
    }

    #[test]
    fn list_command_returns_success_and_prints_catalogue() {
        let args = SoundcardArgs::parse_from(["config-soundcard", "--list"]);
        assert_eq!(run(&args), 0);
    }

    #[test]
    fn detected_command_exits_1_when_nothing_present_in_test_env() {
        let args = SoundcardArgs::parse_from(["config-soundcard", "--detected"]);
        // In the test/CI environment there is no HiFiBerry card, so detection
        // resolves to "Unknown" and the CLI must exit with code 1.
        assert_eq!(run(&args), 1);
    }
}
