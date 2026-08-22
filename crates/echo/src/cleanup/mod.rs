mod local;

pub use local::LocalCleanup;

use echo_core::{Cleanup, CleanupMode, Config, Dictionary, OffCleanup, Rewrite, RulesCleanup};

use crate::settings::file_config;

pub fn from_mode(mode: CleanupMode) -> Box<dyn Cleanup> {
    match mode {
        CleanupMode::Off => Box::new(OffCleanup),
        CleanupMode::Rules => Box::new(RulesCleanup),
        CleanupMode::LocalModel { model } => Box::new(LocalCleanup { bin: model }),
    }
}

fn cleanup_mode(env: Option<&str>, file: &Config) -> CleanupMode {
    echo_core::resolve(
        env.and_then(|raw| CleanupMode::parse(raw).ok()),
        file.cleanup.clone(),
        CleanupMode::Rules,
    )
}

fn cleanup_mode_now() -> CleanupMode {
    cleanup_mode(
        std::env::var("ECHO_CLEANUP").ok().as_deref(),
        &file_config(),
    )
}

fn mode_label(mode: &CleanupMode) -> String {
    match mode {
        CleanupMode::Off => "Off".to_string(),
        CleanupMode::LocalModel { model } => format!("Local · {model}"),
        CleanupMode::Rules => "Rules · fillers and punctuation".to_string(),
    }
}

pub fn from_env() -> Box<dyn Cleanup> {
    from_mode(cleanup_mode_now())
}

/// Clean a transcript. `english` gates the English-specific rules (filler
/// dropping, ASCII punctuation) so they never corrupt another language's
/// output; the dictionary always runs. Off and local modes are unaffected.
pub fn apply(raw: &str, dict: &Dictionary, english: bool) -> Rewrite {
    apply_mode(cleanup_mode_now(), raw, dict, english)
}

fn apply_mode(mode: CleanupMode, raw: &str, dict: &Dictionary, english: bool) -> Rewrite {
    let mode = match (mode, english) {
        (CleanupMode::Rules, false) => CleanupMode::Off,
        (mode, _) => mode,
    };
    from_mode(mode)
        .apply(raw, dict)
        .unwrap_or_else(|_| dict.rewrite(raw))
}

/// The active cleanup mode as a label for status surfaces.
#[must_use]
pub fn mode_name() -> String {
    mode_label(&cleanup_mode_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_and_mode_name_agree() {
        let mode = cleanup_mode_now();
        assert_eq!(mode_name(), mode_label(&mode));
        let rewrite = from_env().apply("um hello", &Dictionary::empty()).unwrap();
        match mode {
            CleanupMode::Off => assert_eq!(rewrite.text, "um hello"),
            CleanupMode::Rules => assert_eq!(rewrite.text, "Hello."),
            CleanupMode::LocalModel { .. } => {}
        }
    }

    #[test]
    fn rules_gate_off_for_non_english() {
        let dict = Dictionary::empty();
        // A Japanese string already ending in 。 gains no ASCII period, and
        // no filler stripping runs, when the session language is not English.
        let rewrite = apply_mode(CleanupMode::Rules, "これはテストです。", &dict, false);
        assert_eq!(rewrite.text, "これはテストです。");
        let rewrite = apply_mode(CleanupMode::Rules, "um hello", &dict, false);
        assert_eq!(rewrite.text, "um hello");
        let rewrite = apply_mode(CleanupMode::Rules, "um hello", &dict, true);
        assert_eq!(rewrite.text, "Hello.");
        // Off and local modes do not change with the gate.
        let rewrite = apply_mode(CleanupMode::Off, "um hello", &dict, false);
        assert_eq!(rewrite.text, "um hello");
    }

    #[test]
    fn dictionary_still_runs_when_rules_are_gated_off() {
        let dir = std::env::temp_dir().join(format!("echo-clean-gate-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut dict = Dictionary::load_from(dir.join("dictionary.json")).unwrap();
        dict.add("button", "Button").unwrap();
        let rewrite = apply_mode(CleanupMode::Rules, "move the button", &dict, false);
        assert_eq!(rewrite.text, "move the Button");
    }

    #[test]
    fn cleanup_mode_prefers_env_then_file_then_rules() {
        let file = Config {
            cleanup: Some(CleanupMode::Off),
            ..Config::default()
        };
        assert_eq!(cleanup_mode(Some("rules"), &file), CleanupMode::Rules);
        assert_eq!(cleanup_mode(None, &file), CleanupMode::Off);
        assert_eq!(cleanup_mode(None, &Config::default()), CleanupMode::Rules);
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
