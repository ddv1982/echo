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
        revision: 0,
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
    let projection = resolved
        .and_then(|run| projection_for_resolved_engine(&run.engine))
        .unwrap_or_else(|| projection_for_support(echo::stt::language_support()));
    build_language_options(projection)
}

pub(crate) fn language_options_for_support(support: echo::stt::LanguageSupport) -> LanguageOptions {
    build_language_options(projection_for_support(support))
}

enum LanguageProjection {
    Whisper {
        model: Option<String>,
        multilingual: bool,
    },
    Parakeet {
        model: Option<String>,
        grouping: LanguageGrouping,
    },
}

#[derive(Clone, Copy)]
enum LanguageGrouping {
    Common,
    All,
}

fn projection_for_resolved_engine(
    engine: &echo::transcribe::ResolvedEngine,
) -> Option<LanguageProjection> {
    match engine {
        echo::transcribe::ResolvedEngine::Whisper {
            model,
            multilingual,
            ..
        } => Some(LanguageProjection::Whisper {
            model: Some(model.clone()),
            multilingual: *multilingual,
        }),
        echo::transcribe::ResolvedEngine::ParakeetTdt06bV3 => Some(LanguageProjection::Parakeet {
            model: Some("tdt-0.6b-v3".to_string()),
            grouping: LanguageGrouping::Common,
        }),
        echo::transcribe::ResolvedEngine::Fake => None,
    }
}

fn projection_for_support(support: echo::stt::LanguageSupport) -> LanguageProjection {
    match support {
        echo::stt::LanguageSupport::WhisperMultilingual => LanguageProjection::Whisper {
            model: None,
            multilingual: true,
        },
        echo::stt::LanguageSupport::WhisperEnglishOnly { model } => LanguageProjection::Whisper {
            model: Some(model),
            multilingual: false,
        },
        echo::stt::LanguageSupport::Parakeet => LanguageProjection::Parakeet {
            model: None,
            grouping: LanguageGrouping::All,
        },
    }
}

fn build_language_options(projection: LanguageProjection) -> LanguageOptions {
    let (mode, model, options) = match projection {
        LanguageProjection::Whisper {
            model,
            multilingual: true,
        } => (
            LanguageMode::Multilingual,
            model,
            echo_core::Language::all()
                .map(|language| language_option(language, LanguageGrouping::Common))
                .collect(),
        ),
        LanguageProjection::Whisper {
            model,
            multilingual: false,
        } => (
            LanguageMode::English,
            model,
            vec![language_option(
                echo_core::Language::ENGLISH,
                LanguageGrouping::Common,
            )],
        ),
        LanguageProjection::Parakeet { model, grouping } => (
            LanguageMode::Parakeet,
            model,
            echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| echo_core::Language::from_code(code))
                .map(|language| language_option(language, grouping))
                .collect(),
        ),
    };
    LanguageOptions {
        mode,
        model,
        options,
    }
}

fn language_option(language: echo_core::Language, grouping: LanguageGrouping) -> LanguageOption {
    LanguageOption {
        code: language.code().to_string(),
        english_name: language.english_name().to_string(),
        group: match grouping {
            LanguageGrouping::Common if ["en", "de", "es", "fr"].contains(&language.code()) => {
                LanguageGroup::Common
            }
            LanguageGrouping::Common | LanguageGrouping::All => LanguageGroup::All,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_run(engine: echo::transcribe::ResolvedEngine) -> echo::transcribe::ResolvedRun {
        echo::transcribe::ResolvedRun {
            engine,
            language: echo_core::LanguageChoice::default(),
        }
    }

    fn assert_common_grouping(options: &[LanguageOption]) {
        for option in options {
            let expected = if ["en", "de", "es", "fr"].contains(&option.code.as_str()) {
                LanguageGroup::Common
            } else {
                LanguageGroup::All
            };
            assert_eq!(
                option.group, expected,
                "unexpected group for {}",
                option.code
            );
        }
    }

    #[test]
    fn resolved_settings_projection_preserves_language_wire_values() {
        let parakeet = resolved_run(echo::transcribe::ResolvedEngine::ParakeetTdt06bV3);
        let parakeet_options = language_options(Some(&parakeet));
        assert_eq!(parakeet_options.mode, LanguageMode::Parakeet);
        assert_eq!(parakeet_options.model.as_deref(), Some("tdt-0.6b-v3"));
        assert_eq!(
            parakeet_options.options.len(),
            echo_core::PARAKEET_LANGUAGES.len()
        );
        for (option, code) in parakeet_options
            .options
            .iter()
            .zip(echo_core::PARAKEET_LANGUAGES.iter().copied())
        {
            let language = echo_core::Language::from_code(code).unwrap();
            assert_eq!(option.code, code);
            assert_eq!(option.english_name, language.english_name());
        }
        assert_common_grouping(&parakeet_options.options);

        let english = resolved_run(echo::transcribe::ResolvedEngine::Whisper {
            model: "base.en".to_string(),
            multilingual: false,
            model_path: None,
        });
        let english_options = language_options(Some(&english));
        assert_eq!(english_options.mode, LanguageMode::English);
        assert_eq!(english_options.model.as_deref(), Some("base.en"));
        assert_eq!(english_options.options.len(), 1);
        assert_eq!(english_options.options[0].code, "en");
        assert_eq!(english_options.options[0].english_name, "english");
        assert_eq!(english_options.options[0].group, LanguageGroup::Common);

        let multilingual = resolved_run(echo::transcribe::ResolvedEngine::Whisper {
            model: "large-v3".to_string(),
            multilingual: true,
            model_path: None,
        });
        let multilingual_options = language_options(Some(&multilingual));
        assert_eq!(multilingual_options.mode, LanguageMode::Multilingual);
        assert_eq!(multilingual_options.model.as_deref(), Some("large-v3"));
        let expected_languages = echo_core::Language::all().collect::<Vec<_>>();
        assert_eq!(multilingual_options.options.len(), expected_languages.len());
        for (option, language) in multilingual_options.options.iter().zip(expected_languages) {
            assert_eq!(option.code, language.code());
            assert_eq!(option.english_name, language.english_name());
        }
        assert_common_grouping(&multilingual_options.options);
    }
}
