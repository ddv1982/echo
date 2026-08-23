mod local;

pub use local::LocalCleanup;

use echo_core::{Cleanup, CleanupMode, OffCleanup, RulesCleanup};

pub fn from_mode(mode: CleanupMode) -> Box<dyn Cleanup> {
    match mode {
        CleanupMode::Off => Box::new(OffCleanup),
        CleanupMode::Rules => Box::new(RulesCleanup),
        CleanupMode::LocalModel { model } => Box::new(LocalCleanup { bin: model }),
    }
}

fn mode_label(mode: &CleanupMode) -> String {
    match mode {
        CleanupMode::Off => "Off".to_string(),
        CleanupMode::LocalModel { model } => format!("Local · {model}"),
        CleanupMode::Rules => "Rules · fillers and punctuation".to_string(),
    }
}

/// The active cleanup mode as a label for status surfaces.
#[must_use]
pub fn mode_name() -> String {
    mode_label(&crate::transcribe::resolved_cleanup_for_process(
        &crate::settings::file_config(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_labels_are_stable() {
        assert_eq!(mode_label(&CleanupMode::Off), "Off");
        assert_eq!(
            mode_label(&CleanupMode::Rules),
            "Rules · fillers and punctuation"
        );
        assert_eq!(
            mode_label(&CleanupMode::LocalModel {
                model: "llama3".into()
            }),
            "Local · llama3"
        );
    }
}
