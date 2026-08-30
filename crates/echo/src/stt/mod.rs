pub mod backend;
mod cache;
mod fake;
mod parakeet;
mod runtime;
mod whisper;
mod whisper_behavior;
mod whisper_gpu;
mod whisper_identity;
mod whisper_plan;
mod whisper_probe;
mod whisper_quarantine;
mod whisper_recovery;

pub use cache::{InstalledModel, ModelCache, ModelInventory, WhisperFamily};
pub use fake::FakeEngine;
pub use parakeet::ParakeetEngine;
pub use runtime::SpeechRuntimeInventory;
pub(crate) use runtime::whisper_runtime_launch;
pub(crate) use whisper::probe_vulkan_runtime_receipt;
pub use whisper::WhisperEngine;

#[must_use]
pub fn whisper_acceleration_factory_default() -> echo_core::WhisperAccelerationPreference {
    echo_core::WhisperAccelerationPreference::Cpu
}

/// Every Vulkan device the installed GPU runtime can see, for the device
/// picker. Returns an empty list when no runtime is installed or no device
/// enumerates, because neither is an error the user needs to read.
#[must_use]
pub fn list_gpu_devices() -> Vec<backend::vulkan::GpuDevice> {
    let Some(runtime) = installed_vulkan_runtime() else {
        return Vec::new();
    };
    let probe = runtime.join("echo-whisper-runtime-probe");
    if !probe.is_file() {
        return Vec::new();
    }
    let backend = backend::vulkan::VulkanBackend::system(
        probe,
        whisper_runtime_launch(&runtime.join("whisper-cli")),
        std::time::Duration::from_secs(15),
    );
    backend
        .enumerate()
        .map(|routes| routes.iter().map(backend::vulkan::LocalVulkanRoute::device).collect())
        .unwrap_or_default()
}

pub(crate) use whisper_gpu::accelerated_engine;

/// Per-user state for the GPU path: the device quarantine and the Mesa shader
/// caches, keyed by accelerator so a driver change starts a fresh one.
#[must_use]
pub(crate) fn whisper_state_dir() -> std::path::PathBuf {
    echo_core::data_dir().join("whisper-gpu")
}

/// The managed GPU runtime payload root, when the component is installed.
#[must_use]
pub fn installed_vulkan_runtime() -> Option<std::path::PathBuf> {
    crate::install::ManagedStore::new(ModelCache::from_env().dir())
        .candidate_root(crate::install::ComponentId::WhisperVulkanRuntime)
        .filter(|root| root.join("whisper-cli").is_file())
}

/// An explicit override wins, then the environment, then the config file,
/// then the factory default.
pub(crate) fn resolved_whisper_acceleration(
    override_preference: Option<echo_core::WhisperAccelerationPreference>,
    file: Option<echo_core::WhisperAccelerationPreference>,
) -> echo_core::WhisperAccelerationPreference {
    override_preference
        .or(std::env::var("ECHO_WHISPER_ACCELERATION")
            .ok()
            .as_deref()
            .and_then(echo_core::WhisperAccelerationPreference::parse))
        .or(file)
        .unwrap_or_else(whisper_acceleration_factory_default)
}
pub use whisper_quarantine::{
    AcceleratorKey, QuarantineReason, QuarantineRecord, MAX_QUARANTINE_LIFETIME_SECS,
};
pub use backend::vulkan::{GpuDevice, VulkanDeviceId};
pub use whisper_identity::{IdentityError as WhisperIdentityError, Sha256Digest, UuidDigest};
pub use whisper_plan::{
    preferred_runtime, VulkanRuntimeSelector, WhisperExecutionPlan, WhisperModelAsset,
    WhisperPlanDecision, WhisperProtocol, WhisperRuntimeCandidate, WhisperRuntimeLaunch,
    WhisperTuning, WhisperTuningOverride,
};
pub use whisper_quarantine::QuarantineStore;
pub use whisper_recovery::RecoveringWhisperEngine;

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
