//! CLI wrapper mirroring `configurator.pimodel:main` (the `config-pimodel` tool).
use clap::Parser;

use crate::api::pimodel::PiModel;

#[derive(Parser, Debug, Default, PartialEq)]
#[command(name = "config-pimodel", about = "Detect and print the Raspberry Pi model")]
pub struct PimodelArgs {}

/// Run the pimodel CLI: prints the detected model name and version.
pub fn run(_args: &PimodelArgs) {
    let model = PiModel::new();
    println!("Model: {}", model.get_model_name());
    println!("Version: {}", model.get_version());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_no_arguments() {
        let args = PimodelArgs::parse_from(["config-pimodel"]);
        assert_eq!(args, PimodelArgs {});
    }
}
