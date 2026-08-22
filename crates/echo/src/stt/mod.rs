mod cache;
mod fake;
mod parakeet;
mod whisper;

pub use cache::ModelCache;
pub use fake::FakeEngine;
pub use parakeet::ParakeetEngine;
pub use whisper::WhisperEngine;

use std::path::PathBuf;

use echo_core::{Config, Engine, EngineChoice, Pcm16kMono, SAMPLE_RATE_HZ};

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
