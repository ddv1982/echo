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

pub fn apply(raw: &str, dict: &Dictionary) -> Rewrite {
    from_env()
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
