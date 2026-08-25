mod cache;
mod fake;
mod parakeet;
mod runtime;
mod whisper;
mod whisper_admission;
mod whisper_plan;
mod whisper_probe;

pub use cache::{InstalledModel, ModelCache, ModelInventory, WhisperFamily};
pub use fake::FakeEngine;
pub use parakeet::ParakeetEngine;
pub(crate) use runtime::whisper_runtime_launch;
pub use runtime::SpeechRuntimeInventory;
pub use whisper::WhisperEngine;
pub use whisper_admission::{
    admission_state_from_bytes, AdmissionDeviceIdentity, AdmissionGates, AdmissionIdentity,
    AdmissionIdentityKey, AdmissionRecord, AdmissionState, AdmissionTuning, AdmissionVerdict,
    QuarantineRecord, MAX_ADMISSION_LIFETIME_SECS, MAX_QUARANTINE_LIFETIME_SECS,
};
pub use whisper_plan::{
    preferred_runtime, WhisperExecutionPlan, WhisperModelAsset, WhisperPlanDecision, WhisperProtocol,
    WhisperRuntimeCandidate, WhisperRuntimeLaunch, WhisperTuning, WhisperTuningOverride,
};

use std::path::PathBuf;

use echo_core::{EngineChoice, LanguageChoice, Pcm16kMono, SAMPLE_RATE_HZ};

use crate::settings::file_config;

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
    let catalog = crate::transcribe::language_catalog(None, &file_config());
    match catalog.selection {
        crate::transcribe::LanguageSelection::AutoOrPinned => LanguageSupport::WhisperMultilingual,
        crate::transcribe::LanguageSelection::EnglishOnly => LanguageSupport::WhisperEnglishOnly {
            model: catalog.model.unwrap_or_default(),
        },
        crate::transcribe::LanguageSelection::AutomaticOnly => LanguageSupport::Parakeet,
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
    let file = file_config();
    let wants = match crate::transcribe::requested_language_for_process(&file)
        .unwrap_or(LanguageChoice::Pinned(echo_core::Language::ENGLISH))
    {
        LanguageChoice::Pinned(echo_core::Language::ENGLISH) => return None,
        LanguageChoice::Pinned(language) => language.english_name().to_string(),
        LanguageChoice::Auto => "automatic detection".to_string(),
    };
    Some(format!(
        "{model} is English-only but the language is set to {wants}. \
         Choose a multilingual model or set the language to English."
    ))
}

#[must_use]
pub fn engine_summary() -> (String, bool) {
    let file = file_config();
    match crate::transcribe::prepare_with_config(crate::transcribe::RunOverrides::default(), &file)
    {
        Ok(prepared) => match &prepared.resolved().engine {
            crate::transcribe::ResolvedEngine::Fake => ("Fake test engine".to_string(), true),
            crate::transcribe::ResolvedEngine::ParakeetTdt06bV3 => {
                ("Parakeet · tdt-0.6b-v3".to_string(), true)
            }
            crate::transcribe::ResolvedEngine::Whisper { model, .. } => {
                let cache = ModelCache::from_env();
                let vad = if SpeechRuntimeInventory::from_cache(&cache)
                    .models
                    .vad
                    .is_empty()
                {
                    "VAD unavailable"
                } else {
                    "VAD on"
                };
                (format!("Whisper · {model} · {vad}"), true)
            }
        },
        Err(crate::transcribe::PrepareError::Configuration(_))
        | Err(crate::transcribe::PrepareError::InvalidRequest(_)) => {
            ("Engine settings need attention".to_string(), false)
        }
        Err(crate::transcribe::PrepareError::EngineMissing(_)) => {
            match crate::transcribe::requested_engine_for_process(&file) {
                EngineChoice::Whisper => ("Whisper setup required".to_string(), false),
                EngineChoice::Parakeet => ("Parakeet setup required".to_string(), false),
                EngineChoice::Fake => ("Fake test engine".to_string(), true),
                EngineChoice::Auto => ("No local engine installed".to_string(), false),
            }
        }
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

fn show_fake_engine(show_fake_env: Option<&str>, engine_env: Option<&str>) -> bool {
    show_fake_env.is_some_and(|value| matches!(value, "1" | "true" | "on"))
        || engine_env == Some("fake")
}

#[must_use]
pub fn engine_availability() -> Vec<EngineAvailability> {
    let cache = ModelCache::from_env();
    let runtime = SpeechRuntimeInventory::from_cache(&cache);
    let whisper_reason = match (
        !runtime.whisper_runtimes.is_empty(),
        runtime.models.whisper.is_empty(),
    ) {
        (true, false) => None,
        (false, _) => Some("whisper-cli is not on PATH".to_string()),
        (true, true) => Some(format!("no Whisper models in {}", cache.dir().display())),
    };
    let parakeet_reason = match (
        runtime.parakeet_binary.is_some(),
        runtime.models.parakeet.is_some(),
    ) {
        (true, true) => None,
        (false, _) => Some("sherpa-onnx-offline is not on PATH".to_string()),
        (true, false) => Some(format!(
            "the parakeet-tdt-0.6b-v3 model files in {} are incomplete",
            cache.dir().display()
        )),
    };
    let mut engines = vec![
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
    ];
    // The fake engine is a smoke-test tool, not a user choice. It joins the
    // shipping selector only when explicitly asked for.
    let show_fake = show_fake_engine(
        std::env::var("ECHO_SHOW_FAKE").ok().as_deref(),
        std::env::var("ECHO_ENGINE").ok().as_deref(),
    );
    if show_fake {
        engines.push(EngineAvailability {
            id: "fake",
            available: true,
            reason: None,
        });
    }
    engines
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
    fn fake_engine_is_hidden_unless_asked_for() {
        assert!(!show_fake_engine(None, None));
        assert!(!show_fake_engine(Some("0"), None));
        assert!(!show_fake_engine(None, Some("whisper")));
        assert!(show_fake_engine(Some("1"), None));
        assert!(show_fake_engine(Some("true"), None));
        assert!(show_fake_engine(None, Some("fake")));
    }
}
