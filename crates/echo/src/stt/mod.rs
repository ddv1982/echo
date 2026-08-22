mod cache;
mod fake;
mod parakeet;
mod whisper;

pub use cache::ModelCache;
pub use fake::FakeEngine;
pub use parakeet::ParakeetEngine;
pub use whisper::WhisperEngine;

use std::path::PathBuf;

use echo_core::{Config, Engine, EngineChoice, LanguageChoice, Pcm16kMono, SAMPLE_RATE_HZ};

use crate::settings::file_config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickedEngine {
    Whisper,
    Parakeet,
    Fake,
}

fn chosen_engine(env: Option<&str>, file: &Config) -> EngineChoice {
    echo_core::resolve(
        env.and_then(EngineChoice::from_env_var),
        file.engine,
        EngineChoice::Auto,
    )
}

fn chosen_engine_now() -> EngineChoice {
    chosen_engine(std::env::var("ECHO_ENGINE").ok().as_deref(), &file_config())
}

/// The language Echo transcribes in: `ECHO_LANGUAGE` wins over the config
/// file, and with neither set the choice is pinned English, matching what
/// Whisper did before Echo had a language concept. Invalid values fall
/// through to the next source rather than failing the session.
#[must_use]
pub fn resolved_language(env: Option<&str>, file: &Config) -> LanguageChoice {
    echo_core::resolve(
        env.and_then(LanguageChoice::parse),
        file.language,
        LanguageChoice::default(),
    )
}

#[must_use]
pub fn language_now() -> LanguageChoice {
    resolved_language(std::env::var("ECHO_LANGUAGE").ok().as_deref(), &file_config())
}

/// What the resolved engine can do about language, for the picker. With no
/// engine installed the full Whisper list shows, since that is the engine a
/// user is about to set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageSupport {
    WhisperMultilingual,
    WhisperEnglishOnly { model: String },
    Parakeet,
}

#[must_use]
pub fn language_support() -> LanguageSupport {
    match pick_engine(chosen_engine_now()) {
        Some(PickedEngine::Whisper) => {
            let engine = WhisperEngine::new();
            match engine.selected_model() {
                Some((path, false)) => LanguageSupport::WhisperEnglishOnly {
                    model: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                },
                Some((_, true)) | None => LanguageSupport::WhisperMultilingual,
            }
        }
        Some(PickedEngine::Parakeet) => LanguageSupport::Parakeet,
        Some(PickedEngine::Fake) | None => LanguageSupport::WhisperMultilingual,
    }
}

/// The mismatch the picker must show before recording: an English-only model
/// combined with a non-English or automatic language. The recorder refuses
/// the same combination; this message names the model and the fix first.
#[must_use]
pub fn language_warning() -> Option<String> {
    let LanguageSupport::WhisperEnglishOnly { model } = language_support() else {
        return None;
    };
    let wants = match language_now() {
        LanguageChoice::Pinned(echo_core::Language::ENGLISH) => return None,
        LanguageChoice::Pinned(language) => language.english_name().to_string(),
        LanguageChoice::Auto => "automatic detection".to_string(),
    };
    Some(format!(
        "{model} is English-only but the language is set to {wants}. \
         Choose a multilingual model or set the language to English."
    ))
}

fn pick_engine(choice: EngineChoice) -> Option<PickedEngine> {
    match choice {
        EngineChoice::Whisper => Some(PickedEngine::Whisper),
        EngineChoice::Parakeet => Some(PickedEngine::Parakeet),
        EngineChoice::Fake => Some(PickedEngine::Fake),
        EngineChoice::Auto => {
            if ParakeetEngine::new().available() {
                Some(PickedEngine::Parakeet)
            } else if WhisperEngine::new().available() {
                Some(PickedEngine::Whisper)
            } else {
                None
            }
        }
    }
}

fn build_engine(pick: PickedEngine) -> Box<dyn Engine> {
    match pick {
        PickedEngine::Whisper => Box::new(WhisperEngine::new()),
        PickedEngine::Parakeet => Box::new(ParakeetEngine::new()),
        PickedEngine::Fake => Box::new(FakeEngine::default()),
    }
}

fn summary_for(pick: Option<PickedEngine>) -> (String, bool) {
    match pick {
        Some(PickedEngine::Fake) => ("Fake test engine".to_string(), true),
        Some(PickedEngine::Whisper) => whisper_summary(),
        Some(PickedEngine::Parakeet) => parakeet_summary(),
        None => ("No local engine installed".to_string(), false),
    }
}

/// The engine `ECHO_ENGINE` or the config file names, or the first installed
/// real engine. `None` when nothing is installed and no engine was requested.
/// The fake engine never runs unless asked for by name.
#[must_use]
pub fn resolve_engine() -> Option<Box<dyn Engine>> {
    Some(build_engine(pick_engine(chosen_engine_now())?))
}

/// Label and readiness of the engine `resolve_engine` would pick, for status
/// surfaces. Projects the same decision so the UI never reports an engine the
/// recorder would not use.
#[must_use]
pub fn engine_summary() -> (String, bool) {
    summary_for(pick_engine(chosen_engine_now()))
}

fn whisper_summary() -> (String, bool) {
    let engine = WhisperEngine::new();
    if engine.available() {
        let vad = if ModelCache::from_env().vad_model().is_some() {
            "VAD on"
        } else {
            "VAD unavailable"
        };
        let model = engine.model_name().unwrap_or("no model selected");
        (format!("Whisper · {model} · {vad}"), true)
    } else {
        ("Whisper setup required".to_string(), false)
    }
}

fn parakeet_summary() -> (String, bool) {
    if ParakeetEngine::new().available() {
        ("Parakeet · tdt-0.6b-v3".to_string(), true)
    } else {
        ("Parakeet setup required".to_string(), false)
    }
}

/// Whether an engine can run right now, with the reason when it cannot. The
/// picker marks unavailable engines rather than hiding them, because "needs
/// sherpa-onnx-offline on PATH" is actionable and a missing row is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAvailability {
    pub id: &'static str,
    pub available: bool,
    pub reason: Option<String>,
}

#[must_use]
pub fn engine_availability() -> Vec<EngineAvailability> {
    let cache = ModelCache::from_env();
    let whisper_reason = match (
        WhisperEngine::binary().is_some(),
        cache.inventory().whisper.is_empty(),
    ) {
        (true, false) => None,
        (false, _) => Some("whisper-cli is not on PATH".to_string()),
        (true, true) => Some(format!("no Whisper models in {}", cache.dir().display())),
    };
    let parakeet_reason = match (
        ParakeetEngine::binary().is_some(),
        cache.parakeet_root().is_some(),
    ) {
        (true, true) => None,
        (false, _) => Some("sherpa-onnx-offline is not on PATH".to_string()),
        (true, false) => Some(format!(
            "the parakeet-tdt-0.6b-v3 model files in {} are incomplete",
            cache.dir().display()
        )),
    };
    vec![
        EngineAvailability {
            id: "whisper",
            available: whisper_reason.is_none(),
            reason: whisper_reason,
        },
        EngineAvailability {
            id: "parakeet",
            available: parakeet_reason.is_none(),
            reason: parakeet_reason,
        },
        EngineAvailability {
            id: "fake",
            available: true,
            reason: None,
        },
    ]
}

fn write_temp_wav(pcm: &Pcm16kMono) -> Result<PathBuf, String> {
    let path =
        std::env::temp_dir().join(format!("echo-stt-{}-{}.wav", std::process::id(), pcm.len()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE_HZ,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).map_err(|err| err.to_string())?;
    for sample in pcm.samples() {
        writer
            .write_sample(*sample)
            .map_err(|err| err.to_string())?;
    }
    writer.finalize().map_err(|err| err.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_engine_and_summary_agree() {
        let pick = pick_engine(chosen_engine_now());
        assert_eq!(engine_summary(), summary_for(pick));
        match pick {
            Some(PickedEngine::Fake) => {
                let engine = resolve_engine().expect("fake engine builds without a model");
                assert_eq!(engine.id().as_str(), "whisper-fake");
            }
            Some(PickedEngine::Whisper) => {
                let (label, _) = engine_summary();
                assert!(label.starts_with("Whisper"), "label={label}");
                if let Some(engine) = resolve_engine() {
                    assert!(engine.id().as_str().starts_with("whisper-"));
                }
            }
            Some(PickedEngine::Parakeet) => {
                let (label, _) = engine_summary();
                assert!(label.starts_with("Parakeet"), "label={label}");
                if let Some(engine) = resolve_engine() {
                    assert_eq!(engine.id().as_str(), "parakeet-tdt-0.6b-v3");
                }
            }
            None => assert!(resolve_engine().is_none()),
        }
    }

    #[test]
    fn config_file_engine_used_when_env_unset() {
        let file = Config {
            engine: Some(EngineChoice::Fake),
            ..Config::default()
        };
        assert_eq!(chosen_engine(None, &file), EngineChoice::Fake);
        assert_eq!(
            pick_engine(chosen_engine(None, &file)),
            Some(PickedEngine::Fake)
        );
        assert_eq!(
            summary_for(Some(PickedEngine::Fake)),
            ("Fake test engine".to_string(), true)
        );
    }
}
