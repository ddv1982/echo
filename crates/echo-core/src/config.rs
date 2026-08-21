use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanupMode;
use crate::paths::{config_path, set_aside_corrupt, write_atomic};

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
    pub hold_key: Option<String>,
    #[serde(default)]
    pub record_seconds: Option<u32>,
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
        let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
        match serde_json::from_str::<Self>(&raw) {
            Ok(config) => Ok(config),
            Err(_) => {
                set_aside_corrupt(path);
                Ok(Self::default())
            }
        }
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
            hold_key: Some("RightCtrl".into()),
            record_seconds: Some(8),
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
        assert_eq!(loaded.hold_key, None);
        assert_eq!(loaded.record_seconds, None);
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
    fn corrupt_file_yields_defaults_and_sibling() {
        let path = scratch_path("corrupt");
        fs::write(&path, "not json at all").unwrap();
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
}
