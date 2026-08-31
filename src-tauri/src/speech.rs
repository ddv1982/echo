use echo_desktop::ipc::{
    EngineAvailability, LanguageGroup, LanguageMode, LanguageOption, LanguageOptions,
    ModelInventory, NextSpeechRun, Readiness, ResolvedSpeechEngine, Settings, SettingsSnapshot,
    TranscriptionSnapshot, WhisperApplicability, WhisperGpuSetup, WhisperModelInfo,
};

#[must_use]
pub(crate) fn model_inventory() -> ModelInventory {
    let cache = echo::stt::ModelCache::from_env();
    let inventory = echo::stt::SpeechRuntimeInventory::from_cache(&cache).models;
    ModelInventory {
        whisper: inventory
            .whisper
            .iter()
            .map(|model| WhisperModelInfo {
                name: model.name.clone(),
                path: model.path.to_string_lossy().into_owned(),
                family: model.family.label().to_string(),
                multilingual: model.multilingual,
                quantisation: model.quantisation.clone(),
                size_bytes: model.size_bytes,
            })
            .collect(),
        vad: inventory
            .vad
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        parakeet: inventory
            .parakeet
            .map(|path| path.to_string_lossy().into_owned()),
        engines: echo::stt::engine_availability()
            .into_iter()
            .map(|engine| EngineAvailability {
                id: engine.id.to_string(),
                available: engine.available,
                reason: engine.reason,
            })
            .collect(),
    }
}

#[must_use]
pub(crate) fn snapshot(
    preferences: Settings,
    file: &echo_core::Config,
    readiness: Readiness,
) -> SettingsSnapshot {
    let resolved = echo::transcribe::resolve_next_run_for_process(file);
    let languages = language_options(resolved.as_ref().ok());
    let next_run = match resolved {
        Ok(run) => NextSpeechRun::Ready {
            engine: resolved_engine(&run.engine),
            language: run.language.as_str().to_string(),
        },
        Err(error) => NextSpeechRun::Unavailable {
            reason: error.to_string(),
        },
    };
    let whisper = whisper_applicability(&preferences, &next_run, &readiness);
    SettingsSnapshot {
        preferences,
        transcription: TranscriptionSnapshot {
            next_run,
            languages,
            models: model_inventory(),
            whisper,
            last_used: crate::status::last_run(),
        },
        readiness,
    }
}

fn resolved_engine(engine: &echo::transcribe::ResolvedEngine) -> ResolvedSpeechEngine {
    match engine {
        echo::transcribe::ResolvedEngine::Whisper {
            model,
            multilingual,
            ..
        } => ResolvedSpeechEngine::Whisper {
            model: model.clone(),
            multilingual: *multilingual,
        },
        echo::transcribe::ResolvedEngine::ParakeetTdt06bV3 => ResolvedSpeechEngine::Parakeet {
            model: "tdt-0.6b-v3".to_string(),
        },
        echo::transcribe::ResolvedEngine::Fake => ResolvedSpeechEngine::Fake,
    }
}

fn language_options(resolved: Option<&echo::transcribe::ResolvedRun>) -> LanguageOptions {
    match resolved.map(|run| &run.engine) {
        Some(echo::transcribe::ResolvedEngine::ParakeetTdt06bV3) => LanguageOptions {
            mode: LanguageMode::Parakeet,
            model: Some("tdt-0.6b-v3".to_string()),
            options: echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| echo_core::Language::from_code(code))
                .map(language_option)
                .collect(),
        },
        Some(echo::transcribe::ResolvedEngine::Whisper {
            model,
            multilingual: false,
            ..
        }) => LanguageOptions {
            mode: LanguageMode::English,
            model: Some(model.clone()),
            options: vec![language_option(echo_core::Language::ENGLISH)],
        },
        Some(echo::transcribe::ResolvedEngine::Whisper {
            model,
            multilingual: true,
            ..
        }) => LanguageOptions {
            mode: LanguageMode::Multilingual,
            model: Some(model.clone()),
            options: echo_core::Language::all().map(language_option).collect(),
        },
        Some(echo::transcribe::ResolvedEngine::Fake) | None => fallback_language_options(),
    }
}

fn fallback_language_options() -> LanguageOptions {
    match echo::stt::language_support() {
        echo::stt::LanguageSupport::WhisperMultilingual => LanguageOptions {
            mode: LanguageMode::Multilingual,
            model: None,
            options: echo_core::Language::all().map(language_option).collect(),
        },
        echo::stt::LanguageSupport::WhisperEnglishOnly { model } => LanguageOptions {
            mode: LanguageMode::English,
            model: Some(model),
            options: vec![language_option(echo_core::Language::ENGLISH)],
        },
        echo::stt::LanguageSupport::Parakeet => LanguageOptions {
            mode: LanguageMode::Parakeet,
            model: Some("tdt-0.6b-v3".to_string()),
            options: echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| echo_core::Language::from_code(code))
                .map(language_option)
                .collect(),
        },
    }
}

fn language_option(language: echo_core::Language) -> LanguageOption {
    LanguageOption {
        code: language.code().to_string(),
        english_name: language.english_name().to_string(),
        group: if ["en", "de", "es", "fr"].contains(&language.code()) {
            LanguageGroup::Common
        } else {
            LanguageGroup::All
        },
    }
}

fn whisper_applicability(
    preferences: &Settings,
    next_run: &NextSpeechRun,
    readiness: &Readiness,
) -> WhisperApplicability {
    let NextSpeechRun::Ready {
        engine: ResolvedSpeechEngine::Whisper { .. },
        ..
    } = next_run
    else {
        return WhisperApplicability::Deferred {
            reason: "GPU preference saved for Whisper".to_string(),
        };
    };
    if preferences.whisper_acceleration.effective != "gpu" {
        return WhisperApplicability::Applicable {
            gpu: WhisperGpuSetup::NotRequested,
        };
    }
    let prerequisite = [
        echo_desktop::ipc::ComponentId::WhisperRuntime,
        echo_desktop::ipc::ComponentId::WhisperVulkanRuntime,
    ]
    .into_iter()
    .filter_map(|id| {
        readiness
            .components
            .iter()
            .find(|component| component.id == id)
    })
    .find(|component| {
        !matches!(
            &component.managed,
            echo_desktop::ipc::ManagedComponentState::Ready { .. }
        )
    });
    match prerequisite {
        Some(component)
            if matches!(
                &component.managed,
                echo_desktop::ipc::ManagedComponentState::Unsupported { .. }
            ) =>
        {
            WhisperApplicability::Applicable {
                gpu: WhisperGpuSetup::Unsupported {
                    component: component.clone(),
                },
            }
        }
        Some(component) => WhisperApplicability::Applicable {
            gpu: WhisperGpuSetup::NeedsInstall {
                component: component.clone(),
            },
        },
        None => WhisperApplicability::Applicable {
            gpu: WhisperGpuSetup::Ready,
        },
    }
}
