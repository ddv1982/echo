use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanupMode;
use crate::engine::WhisperAccelerationPreference;
use crate::language::LanguageChoice;
use crate::paths::{config_path, set_aside_corrupt, write_atomic};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "snake_case"
)]
pub enum MicrophoneSelection {
    Device { id: String, last_seen_label: String },
    LegacyName { name: String },
}

impl<'de> Deserialize<'de> for MicrophoneSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            tag = "kind",
            rename_all = "kebab-case",
            rename_all_fields = "snake_case"
        )]
        enum Tagged {
            Device { id: String, last_seen_label: String },
            LegacyName { name: String },
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Legacy(String),
            Tagged(Tagged),
        }

        Ok(match Stored::deserialize(deserializer)? {
            Stored::Legacy(name) => Self::LegacyName { name },
            Stored::Tagged(Tagged::Device {
                id,
                last_seen_label,
            }) => Self::Device {
                id,
                last_seen_label,
            },
            Stored::Tagged(Tagged::LegacyName { name }) => Self::LegacyName { name },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineChoice {
    Whisper,
    Parakeet,
    Fake,
    Auto,
}

impl EngineChoice {
    #[must_use]
    pub fn from_env_var(raw: &str) -> Option<Self> {
        match raw {
            "whisper" => Some(Self::Whisper),
            "parakeet" => Some(Self::Parakeet),
            "fake" => Some(Self::Fake),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub engine: Option<EngineChoice>,
    #[serde(default)]
    pub whisper_model: Option<String>,
    #[serde(default)]
    pub cleanup: Option<CleanupMode>,
    #[serde(default)]
    pub hud: Option<bool>,
    #[serde(default)]
    pub record_seconds: Option<u32>,
    #[serde(default)]
    pub microphone: Option<MicrophoneSelection>,
    #[serde(default)]
    pub language: Option<LanguageChoice>,
    #[serde(default)]
    pub whisper_acceleration: Option<WhisperAccelerationPreference>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        Self::load_from(config_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read(path).map_err(|err| err.to_string())?;
        match serde_json::from_slice::<Self>(&raw) {
            Ok(config) => Ok(config),
            Err(_) => {
                set_aside_corrupt(path);
                Ok(Self::default())
            }
        }
    }

    pub fn load_read_only() -> Result<Self, String> {
        Self::load_from_read_only(config_path())
    }

    pub fn load_from_read_only(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read(path).map_err(|err| err.to_string())?;
        Ok(serde_json::from_slice::<Self>(&raw).unwrap_or_default())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(config_path())
    }

    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        write_atomic(path.as_ref(), raw.as_bytes())
    }
}

#[must_use]
pub fn resolve<T>(env: Option<T>, file: Option<T>, default: T) -> T {
    match (env, file) {
        (Some(value), _) => value,
        (None, Some(value)) => value,
        (None, None) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch_path(label: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "echo-config-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = scratch_path("roundtrip");
        let original = Config {
            engine: Some(EngineChoice::Whisper),
            whisper_model: Some("base.en".into()),
            cleanup: Some(CleanupMode::LocalModel {
                model: "llama3".into(),
            }),
            hud: Some(true),
            record_seconds: Some(8),
            microphone: Some(MicrophoneSelection::Device {
                id: "alsa:hw:USB".into(),
                last_seen_label: "USB Mic".into(),
            }),
            language: Some(LanguageChoice::Auto),
            whisper_acceleration: Some(WhisperAccelerationPreference::Gpu),
        };
        original.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), original);
    }

    #[test]
    fn missing_field_is_none() {
        let path = scratch_path("partial");
        fs::write(&path, r#"{"engine":"fake"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.engine, Some(EngineChoice::Fake));
        assert_eq!(loaded.whisper_model, None);
        assert_eq!(loaded.cleanup, None);
        assert_eq!(loaded.hud, None);
        assert_eq!(loaded.record_seconds, None);
        assert_eq!(loaded.microphone, None);
        assert_eq!(loaded.language, None);
        assert_eq!(loaded.whisper_acceleration, None);
    }

    #[test]
    fn legacy_microphone_name_loads_without_becoming_an_id() {
        let path = scratch_path("legacy-microphone");
        fs::write(&path, r#"{"microphone":"USB Mic"}"#).unwrap();
        assert_eq!(
            Config::load_from(&path).unwrap().microphone,
            Some(MicrophoneSelection::LegacyName {
                name: "USB Mic".into()
            })
        );
    }

    #[test]
    fn stable_microphone_id_round_trips_with_last_seen_label() {
        let path = scratch_path("stable-microphone");
        let selection = MicrophoneSelection::Device {
            id: "alsa:hw:CARD=USB,DEV=0".into(),
            last_seen_label: "USB Mic".into(),
        };
        let config = Config {
            microphone: Some(selection.clone()),
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(
            Config::load_from(&path).unwrap().microphone,
            Some(selection)
        );
        let raw = fs::read_to_string(path).unwrap();
        assert!(raw.contains(r#""kind": "device""#));
        assert!(raw.contains(r#""last_seen_label": "USB Mic""#));
    }

    #[test]
    fn unknown_field_is_ignored() {
        let path = scratch_path("unknown");
        fs::write(&path, r#"{"engine":"whisper","future_knob":true}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.engine, Some(EngineChoice::Whisper));
        assert_eq!(
            loaded,
            Config {
                engine: Some(EngineChoice::Whisper),
                ..Config::default()
            }
        );
    }

    #[test]
    fn obsolete_shortcuts_are_ignored_and_removed_on_save() {
        let path = scratch_path("old-shortcut");
        fs::write(
            &path,
            r#"{"engine":"fake","hold_key":"not+a+valid+shortcut","toggle_shortcut":"also invalid","record_seconds":8}"#,
        )
        .unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.engine, Some(EngineChoice::Fake));
        assert_eq!(loaded.record_seconds, Some(8));
        loaded.save_to(&path).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("hold_key"));
        assert!(!saved.contains("toggle_shortcut"));
        assert_eq!(Config::load_from(&path).unwrap(), loaded);
    }

    #[test]
    fn corrupt_file_yields_defaults_and_sibling() {
        let path = scratch_path("corrupt");
        fs::write(&path, "not json at all").unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn invalid_utf8_file_yields_defaults_and_sibling() {
        let path = scratch_path("invalid-utf8");
        fs::write(&path, [0xff, 0xfe, b'{']).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn resolve_prefers_env_then_file_then_default() {
        assert_eq!(resolve(Some(1), Some(2), 3), 1);
        assert_eq!(resolve(None, Some(2), 3), 2);
        assert_eq!(resolve::<i32>(None, None, 3), 3);
    }

    #[test]
    fn invalid_engine_yields_defaults_and_sibling() {
        let path = scratch_path("invalid-engine");
        fs::write(&path, r#"{"engine":"Whisper"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "invalid engine should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn read_only_corrupt_config_does_not_move_or_rewrite_it() {
        let path = scratch_path("read-only-corrupt");
        fs::write(&path, "not json").unwrap();
        assert_eq!(
            Config::load_from_read_only(&path).unwrap(),
            Config::default()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
        assert!(!path.with_file_name("config.json.corrupt").exists());
    }
}
