use std::path::{Path, PathBuf};

use echo_core::{
    CleanupError, CleanupMode, Config, DecodeOptions, Dictionary, Engine, EngineChoice,
    EngineError, EngineId, Language, LanguageChoice, Pcm16kMono, RecognitionHints, RunDetail,
    WhisperAccelerationPreference,
};

use crate::install::ManagedPath;
use crate::stt::{
    local_whisper_engine_from_process, preferred_runtime, production_whisper_decision,
    resolved_whisper_acceleration, whisper_runtime_launch, FakeEngine, ModelCache, ParakeetEngine,
    QuarantineStore, RecoveringWhisperEngine, SpeechRuntimeInventory, WhisperEngine,
    WhisperExecutionPlan, WhisperModelAsset, WhisperTuningOverride,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOverrides {
    pub engine: Option<EngineChoice>,
    pub whisper_model: Option<String>,
    pub language: Option<LanguageChoice>,
    pub whisper_tuning: Option<WhisperTuningOverride>,
    pub whisper_force_cpu: bool,
    pub whisper_acceleration: Option<WhisperAccelerationPreference>,
    pub whisper_vulkan_driver_files: Option<PathBuf>,
    pub whisper_mesa_shader_cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvOptions {
    pub engine: Option<EngineChoice>,
    pub whisper_model: Option<String>,
    pub language: Option<LanguageChoice>,
    pub cleanup: Option<CleanupMode>,
}

impl EnvOptions {
    #[must_use]
    pub fn read() -> Self {
        Self {
            engine: std::env::var("ECHO_ENGINE")
                .ok()
                .as_deref()
                .and_then(EngineChoice::from_env_var),
            whisper_model: std::env::var("ECHO_WHISPER_MODEL")
                .ok()
                .filter(|name| !name.is_empty()),
            language: std::env::var("ECHO_LANGUAGE")
                .ok()
                .as_deref()
                .and_then(LanguageChoice::parse),
            cleanup: std::env::var("ECHO_CLEANUP")
                .ok()
                .as_deref()
                .and_then(|raw| CleanupMode::parse(raw).ok()),
        }
    }
}

#[must_use]
pub(crate) fn requested_engine_for_process(file: &Config) -> EngineChoice {
    EnvOptions::read()
        .engine
        .or(file.engine)
        .unwrap_or(EngineChoice::Auto)
}

#[must_use]
pub(crate) fn requested_language_for_process(file: &Config) -> Option<LanguageChoice> {
    EnvOptions::read().language.or(file.language)
}

#[must_use]
pub(crate) fn resolved_cleanup_for_process(file: &Config) -> CleanupMode {
    EnvOptions::read()
        .cleanup
        .or_else(|| file.cleanup.clone())
        .unwrap_or(CleanupMode::Rules)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperModelCapability {
    pub name: String,
    pub multilingual: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineAvailabilitySnapshot {
    pub whisper_binary: bool,
    pub whisper_models: Vec<WhisperModelCapability>,
    pub parakeet: bool,
}

impl EngineAvailabilitySnapshot {
    fn from_process(
        cache: &ModelCache,
        runtime: &SpeechRuntimeInventory,
        model_candidates: &[String],
    ) -> Self {
        let inventory = &runtime.models;
        let best = inventory.best_whisper().map(|model| model.name.clone());
        let mut whisper_models = inventory
            .whisper
            .iter()
            .map(|model| WhisperModelCapability {
                name: model.name.clone(),
                multilingual: model.multilingual,
                path: Some(model.path.clone()),
            })
            .collect::<Vec<_>>();
        for name in model_candidates {
            if whisper_models.iter().any(|model| model.name == *name) {
                continue;
            }
            if let Some((_, multilingual)) =
                WhisperEngine::configured(cache.clone(), name).selected_model()
            {
                whisper_models.push(WhisperModelCapability {
                    name: name.clone(),
                    multilingual,
                    path: WhisperEngine::configured(cache.clone(), name)
                        .selected_model()
                        .map(|(path, _)| path),
                });
            }
        }
        if let Some(best) = best {
            if let Some(index) = whisper_models.iter().position(|model| model.name == best) {
                let best = whisper_models.remove(index);
                whisper_models.push(best);
            }
        }
        Self {
            whisper_binary: !runtime.whisper_runtimes.is_empty(),
            whisper_models,
            parakeet: runtime.parakeet_binary.is_some() && runtime.models.parakeet.is_some(),
        }
    }

    fn best_whisper(&self) -> Option<&WhisperModelCapability> {
        self.whisper_models.last()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedEngine {
    Whisper {
        model: String,
        multilingual: bool,
        model_path: Option<PathBuf>,
    },
    ParakeetTdt06bV3,
    Fake,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRun {
    pub engine: ResolvedEngine,
    pub language: LanguageChoice,
    pub cleanup: CleanupMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    InvalidRequest(String),
    Configuration(String),
    EngineMissing(String),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message)
            | Self::Configuration(message)
            | Self::EngineMissing(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PrepareError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Default,
    File,
    Environment,
    Override,
}

struct RequestedRun {
    engine: EngineChoice,
    engine_source: Source,
    language: Option<LanguageChoice>,
    language_source: Option<Source>,
    whisper_model: Option<String>,
    model_source: Option<Source>,
    cleanup: CleanupMode,
}

impl RequestedRun {
    fn new(overrides: &RunOverrides, env: &EnvOptions, file: &Config) -> Self {
        let (engine, engine_source) = if let Some(engine) = overrides.engine {
            (engine, Source::Override)
        } else if let Some(engine) = env.engine {
            (engine, Source::Environment)
        } else if let Some(engine) = file.engine {
            (engine, Source::File)
        } else {
            (EngineChoice::Auto, Source::Default)
        };
        let (language, language_source) = if let Some(language) = overrides.language {
            (Some(language), Some(Source::Override))
        } else if let Some(language) = env.language {
            (Some(language), Some(Source::Environment))
        } else if let Some(language) = file.language {
            (Some(language), Some(Source::File))
        } else {
            (None, None)
        };
        let (whisper_model, model_source) = if let Some(model) = &overrides.whisper_model {
            (Some(model.clone()), Some(Source::Override))
        } else if let Some(model) = &env.whisper_model {
            (Some(model.clone()), Some(Source::Environment))
        } else if let Some(model) = &file.whisper_model {
            (Some(model.clone()), Some(Source::File))
        } else {
            (None, None)
        };
        Self {
            engine,
            engine_source,
            language,
            language_source,
            whisper_model,
            model_source,
            cleanup: env
                .cleanup
                .clone()
                .or_else(|| file.cleanup.clone())
                .unwrap_or(CleanupMode::Rules),
        }
    }

    fn strongest_whisper_constraint(&self) -> Option<Source> {
        let language = self
            .language
            .filter(|choice| matches!(choice, LanguageChoice::Pinned(_)))
            .and(self.language_source);
        language.max(self.model_source)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParakeetDecision {
    UseParakeet,
    UseWhisper,
    Conflict(Source),
}

fn parakeet_decision(requested: &RequestedRun) -> ParakeetDecision {
    let Some(constraint_source) = requested.strongest_whisper_constraint() else {
        return ParakeetDecision::UseParakeet;
    };
    if constraint_source > requested.engine_source {
        ParakeetDecision::UseWhisper
    } else if constraint_source == requested.engine_source && constraint_source != Source::File {
        ParakeetDecision::Conflict(constraint_source)
    } else {
        ParakeetDecision::UseParakeet
    }
}

fn projects_parakeet(requested: &RequestedRun, parakeet_available: bool) -> bool {
    match requested.engine {
        EngineChoice::Parakeet => {
            matches!(parakeet_decision(requested), ParakeetDecision::UseParakeet)
        }
        EngineChoice::Auto => {
            requested.strongest_whisper_constraint().is_none() && parakeet_available
        }
        EngineChoice::Whisper | EngineChoice::Fake => false,
    }
}

pub fn resolve_run(
    overrides: &RunOverrides,
    env: &EnvOptions,
    file: &Config,
    available: &EngineAvailabilitySnapshot,
) -> Result<ResolvedRun, PrepareError> {
    let requested = RequestedRun::new(overrides, env, file);

    let selected = match requested.engine {
        EngineChoice::Fake => ResolvedEngine::Fake,
        EngineChoice::Parakeet => match parakeet_decision(&requested) {
            ParakeetDecision::UseWhisper => {
                resolve_whisper(requested.whisper_model.as_deref(), available)?
            }
            ParakeetDecision::Conflict(source) => {
                let message = if requested.model_source == Some(source) {
                    "a Whisper model cannot be combined with the Parakeet engine"
                } else {
                    "Parakeet supports automatic language selection only"
                };
                let error = if source == Source::Override {
                    PrepareError::InvalidRequest(message.to_string())
                } else {
                    PrepareError::Configuration(message.to_string())
                };
                return Err(error);
            }
            ParakeetDecision::UseParakeet => {
                if !available.parakeet {
                    return Err(PrepareError::EngineMissing(
                        "Parakeet engine or model is missing".to_string(),
                    ));
                }
                ResolvedEngine::ParakeetTdt06bV3
            }
        },
        EngineChoice::Whisper => resolve_whisper(requested.whisper_model.as_deref(), available)?,
        EngineChoice::Auto => {
            let must_use_whisper = requested.strongest_whisper_constraint().is_some();
            if must_use_whisper {
                resolve_whisper(requested.whisper_model.as_deref(), available)?
            } else if available.parakeet {
                ResolvedEngine::ParakeetTdt06bV3
            } else {
                resolve_whisper(requested.whisper_model.as_deref(), available)?
            }
        }
    };

    let language = match &selected {
        ResolvedEngine::ParakeetTdt06bV3 => LanguageChoice::Auto,
        ResolvedEngine::Fake => requested.language.unwrap_or_default(),
        ResolvedEngine::Whisper { multilingual, .. } => requested.language.unwrap_or({
            if *multilingual {
                LanguageChoice::Auto
            } else {
                LanguageChoice::Pinned(Language::ENGLISH)
            }
        }),
    };
    if let ResolvedEngine::Whisper {
        model,
        multilingual: false,
        ..
    } = &selected
    {
        if !matches!(language, LanguageChoice::Pinned(Language::ENGLISH)) {
            let message =
                format!("{model} is an English-only model; choose English or a multilingual model");
            if overrides.language.is_some() || overrides.whisper_model.is_some() {
                return Err(PrepareError::InvalidRequest(message));
            }
            return Err(PrepareError::Configuration(message));
        }
    }
    Ok(ResolvedRun {
        engine: selected,
        language,
        cleanup: requested.cleanup,
    })
}

fn resolve_whisper(
    requested_model: Option<&str>,
    available: &EngineAvailabilitySnapshot,
) -> Result<ResolvedEngine, PrepareError> {
    if !available.whisper_binary {
        return Err(PrepareError::EngineMissing(
            "whisper-cli is not on PATH".to_string(),
        ));
    }
    let model = match requested_model {
        Some(name) => available
            .whisper_models
            .iter()
            .find(|model| model.name == name),
        None => available.best_whisper(),
    }
    .ok_or_else(|| {
        PrepareError::EngineMissing(match requested_model {
            Some(name) => format!("Whisper model {name} is not installed"),
            None => "no Whisper model is installed".to_string(),
        })
    })?;
    Ok(ResolvedEngine::Whisper {
        model: model.name.clone(),
        multilingual: model.multilingual,
        model_path: model.path.clone(),
    })
}

pub struct PreparedTranscription {
    resolved: ResolvedRun,
    engine: Box<dyn Engine>,
    _managed_paths: Vec<ManagedPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupPolicy {
    Strict,
    DictionaryFallback,
    Skip,
}

impl PreparedTranscription {
    #[must_use]
    pub fn resolved(&self) -> &ResolvedRun {
        &self.resolved
    }

    pub fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        dictionary: &Dictionary,
        cleanup_policy: CleanupPolicy,
    ) -> Result<CompletedTranscription, TranscriptionError> {
        let hints = match self.resolved.engine {
            ResolvedEngine::Whisper { .. } => RecognitionHints::from_dictionary(dictionary),
            ResolvedEngine::ParakeetTdt06bV3 | ResolvedEngine::Fake => RecognitionHints::default(),
        };
        let hint_count = hints.terms().len();
        let options = DecodeOptions {
            language: self.resolved.language,
            hints,
        };
        let transcript = self
            .engine
            .transcribe(pcm, &options)
            .map_err(TranscriptionError::Engine)?;
        let english = self
            .resolved
            .language
            .permits_english_rules(transcript.detail.language.as_deref());
        let rewrite = match cleanup_policy {
            CleanupPolicy::Skip => echo_core::Rewrite {
                text: transcript.raw.clone(),
            },
            CleanupPolicy::Strict | CleanupPolicy::DictionaryFallback => {
                let cleanup = effective_cleanup(self.resolved.cleanup.clone(), english);
                match cleanup.apply(&transcript.raw, dictionary) {
                    Ok(rewrite) => rewrite,
                    Err(_) if cleanup_policy == CleanupPolicy::DictionaryFallback => {
                        dictionary.rewrite(&transcript.raw)
                    }
                    Err(error) => return Err(TranscriptionError::Cleanup(error)),
                }
            }
        };
        Ok(CompletedTranscription {
            raw: transcript.raw,
            text: rewrite.text,
            engine: transcript.engine,
            audio_ms: transcript.audio_ms,
            infer_ms: transcript.infer_ms,
            detail: transcript.detail,
            requested_language: self.resolved.language,
            hint_count,
        })
    }
}

fn effective_cleanup(mode: CleanupMode, english: bool) -> Box<dyn echo_core::Cleanup> {
    let mode = match (mode, english) {
        (CleanupMode::Rules, false) => CleanupMode::Off,
        (mode, _) => mode,
    };
    crate::cleanup::from_mode(mode)
}

pub fn prepare_with_config(
    overrides: RunOverrides,
    file: &Config,
) -> Result<PreparedTranscription, PrepareError> {
    let env = EnvOptions::read();
    let cache = ModelCache::from_env();
    let runtime = SpeechRuntimeInventory::from_cache(&cache);
    let model_candidates = [
        overrides.whisper_model.clone(),
        env.whisper_model.clone(),
        file.whisper_model.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let available = EngineAvailabilitySnapshot::from_process(&cache, &runtime, &model_candidates);
    let resolved = resolve_run(&overrides, &env, file, &available)?;
    if (overrides.whisper_tuning.is_some()
        || overrides.whisper_force_cpu
        || overrides.whisper_vulkan_driver_files.is_some()
        || overrides.whisper_mesa_shader_cache_dir.is_some())
        && !matches!(resolved.engine, ResolvedEngine::Whisper { .. })
    {
        return Err(PrepareError::InvalidRequest(
            "Whisper-only performance options require the Whisper engine".to_string(),
        ));
    }
    let (engine, managed_paths): (Box<dyn Engine>, Vec<ManagedPath>) = match &resolved.engine {
        ResolvedEngine::Whisper {
            model,
            multilingual,
            model_path,
        } => {
            let runtime_candidate = preferred_runtime(&runtime.whisper_runtimes)
                .cloned()
                .ok_or_else(|| {
                    PrepareError::EngineMissing("whisper-cli is not installed".to_string())
                })?;
            let model_path = model_path.clone().ok_or_else(|| {
                PrepareError::EngineMissing(format!("Whisper model {model} is not installed"))
            })?;
            let vad = runtime.models.vad.first().cloned();
            let mut selected = vec![runtime_candidate.cli.clone()];
            if let Some(server) = &runtime_candidate.server {
                selected.push(server.clone());
            }
            selected.push(model_path.clone());
            if let Some(vad) = &vad {
                selected.push(vad.clone());
            }
            let locked = runtime
                .lock_selected(&selected)
                .map_err(PrepareError::EngineMissing)?;
            let mut paths = locked.paths.into_iter();
            let mut locked_runtime = runtime_candidate;
            locked_runtime.cli = paths.next().expect("selected runtime has a CLI");
            locked_runtime.launch = whisper_runtime_launch(&locked_runtime.cli);
            locked_runtime.launch.vulkan_driver_files = canonical_launch_path(
                overrides.whisper_vulkan_driver_files.as_deref(),
                "Whisper Vulkan driver manifest",
                false,
            )?;
            locked_runtime.launch.mesa_shader_cache_dir = canonical_launch_path(
                overrides.whisper_mesa_shader_cache_dir.as_deref(),
                "Whisper Mesa shader cache",
                true,
            )?;
            locked_runtime.server = locked_runtime
                .server
                .as_ref()
                .map(|_| paths.next().expect("selected server was locked"));
            let locked_model = paths.next().expect("selected model was locked");
            let resolved_vad = vad.map(|_| paths.next().expect("selected VAD was locked"));
            let mut plan = WhisperExecutionPlan::one_shot(
                locked_runtime,
                WhisperModelAsset {
                    name: model.clone(),
                    path: locked_model,
                    multilingual: *multilingual,
                },
                resolved_vad,
            );
            if let Some(overrides) = overrides.whisper_tuning {
                plan.tuning = overrides.apply(plan.tuning);
            }
            let preference = resolved_whisper_acceleration(
                overrides.whisper_acceleration,
                file.whisper_acceleration,
            );
            plan.force_cpu = overrides.whisper_force_cpu
                || preference == WhisperAccelerationPreference::Cpu
                || (plan.runtime.source == echo_core::WhisperRuntimeSource::Managed
                    && plan.runtime.backend == echo_core::WhisperRuntimeBackend::Cpu);
            let can_accelerate = plan.runtime.source == echo_core::WhisperRuntimeSource::Managed
                && plan.runtime.backend == echo_core::WhisperRuntimeBackend::Cpu
                && preference != WhisperAccelerationPreference::Cpu
                && overrides.whisper_tuning.is_none()
                && !overrides.whisper_force_cpu
                && overrides.whisper_vulkan_driver_files.is_none()
                && overrides.whisper_mesa_shader_cache_dir.is_none();
            let engine: Box<dyn Engine> = if can_accelerate {
                local_whisper_engine_from_process(plan.clone(), preference).unwrap_or_else(|| {
                    production_whisper_decision(plan.clone()).map_or_else(
                        || Box::new(WhisperEngine::with_plan(plan)) as Box<dyn Engine>,
                        |decision| {
                            let quarantine = QuarantineStore::at(
                                echo_core::data_dir().join("whisper-quarantine.json"),
                            );
                            Box::new(RecoveringWhisperEngine::new(decision, quarantine))
                                as Box<dyn Engine>
                        },
                    )
                })
            } else {
                Box::new(WhisperEngine::with_plan(plan))
            };
            (engine, locked.leases)
        }
        ResolvedEngine::ParakeetTdt06bV3 => {
            let binary = runtime.parakeet_binary.clone().ok_or_else(|| {
                PrepareError::EngineMissing("sherpa-onnx-offline is not installed".to_string())
            })?;
            let model = runtime.models.parakeet.clone().ok_or_else(|| {
                PrepareError::EngineMissing("Parakeet model is not installed".to_string())
            })?;
            let locked = runtime
                .lock_selected(&[binary, model])
                .map_err(PrepareError::EngineMissing)?;
            (
                Box::new(ParakeetEngine::with_paths(
                    locked.paths[0].clone(),
                    locked.paths[1].clone(),
                )),
                locked.leases,
            )
        }
        ResolvedEngine::Fake => (Box::new(FakeEngine::default()), Vec::new()),
    };
    Ok(PreparedTranscription {
        resolved,
        engine,
        _managed_paths: managed_paths,
    })
}

fn canonical_launch_path(
    path: Option<&Path>,
    label: &str,
    directory: bool,
) -> Result<Option<PathBuf>, PrepareError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let resolved = path.canonicalize().map_err(|error| {
        PrepareError::InvalidRequest(format!("{label} is unavailable: {error}"))
    })?;
    if directory != resolved.is_dir() {
        return Err(PrepareError::InvalidRequest(format!(
            "{label} must be a {}",
            if directory { "directory" } else { "file" }
        )));
    }
    Ok(Some(resolved))
}

pub fn transcribe_file(
    path: &Path,
    prepared: &PreparedTranscription,
    dictionary: &Dictionary,
    cleanup_policy: CleanupPolicy,
) -> Result<CompletedTranscription, TranscriptionError> {
    let capture = crate::audio::load_wav(path).map_err(TranscriptionError::Audio)?;
    prepared.transcribe(&capture.pcm, dictionary, cleanup_policy)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletedTranscription {
    pub raw: String,
    pub text: String,
    pub engine: EngineId,
    pub audio_ms: u64,
    pub infer_ms: u64,
    pub detail: RunDetail,
    pub requested_language: LanguageChoice,
    pub hint_count: usize,
}

#[derive(Debug)]
pub enum TranscriptionError {
    Audio(crate::audio::AudioError),
    Engine(EngineError),
    Cleanup(CleanupError),
}

impl std::fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(error) => error.fmt(f),
            Self::Engine(error) => error.fmt(f),
            Self::Cleanup(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TranscriptionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSelection {
    AutoOrPinned,
    EnglishOnly,
    AutomaticOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageCatalog {
    pub engine: &'static str,
    pub model: Option<String>,
    pub selection: LanguageSelection,
    pub languages: Vec<Language>,
}

pub fn language_catalog(engine: Option<EngineChoice>, file: &Config) -> LanguageCatalog {
    let cache = ModelCache::from_env();
    let runtime = SpeechRuntimeInventory::from_cache(&cache);
    let env = EnvOptions::read();
    let requested = RequestedRun::new(
        &RunOverrides {
            engine,
            ..RunOverrides::default()
        },
        &env,
        file,
    );
    let parakeet_available = runtime.models.parakeet.is_some() && runtime.parakeet_binary.is_some();
    let parakeet = projects_parakeet(&requested, parakeet_available);
    if parakeet {
        return LanguageCatalog {
            engine: "parakeet",
            model: Some("tdt-0.6b-v3".to_string()),
            selection: LanguageSelection::AutomaticOnly,
            languages: echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| Language::from_code(code))
                .collect(),
        };
    }
    let model = requested.whisper_model.or_else(|| {
        runtime
            .models
            .best_whisper()
            .map(|model| model.name.clone())
    });
    let multilingual = model.as_deref().and_then(|name| {
        runtime
            .models
            .whisper
            .iter()
            .find(|model| model.name == name)
            .map(|model| model.multilingual)
            .or_else(|| {
                WhisperEngine::configured(cache.clone(), name)
                    .selected_model()
                    .map(|(_, multilingual)| multilingual)
            })
    });
    if multilingual == Some(false) {
        LanguageCatalog {
            engine: "whisper",
            model,
            selection: LanguageSelection::EnglishOnly,
            languages: vec![Language::ENGLISH],
        }
    } else {
        LanguageCatalog {
            engine: "whisper",
            model,
            selection: LanguageSelection::AutoOrPinned,
            languages: Language::all().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available(multilingual: bool, parakeet: bool) -> EngineAvailabilitySnapshot {
        EngineAvailabilitySnapshot {
            whisper_binary: true,
            whisper_models: vec![WhisperModelCapability {
                name: if multilingual { "small" } else { "base.en" }.to_string(),
                multilingual,
                path: None,
            }],
            parakeet,
        }
    }

    fn german() -> LanguageChoice {
        LanguageChoice::Pinned(Language::from_code("de").unwrap())
    }

    #[test]
    fn auto_pin_and_cli_model_force_whisper() {
        let resolved = resolve_run(
            &RunOverrides {
                language: Some(german()),
                ..RunOverrides::default()
            },
            &EnvOptions::default(),
            &Config::default(),
            &available(true, true),
        )
        .unwrap();
        assert!(matches!(resolved.engine, ResolvedEngine::Whisper { .. }));
        assert_eq!(resolved.language, german());

        let resolved = resolve_run(
            &RunOverrides {
                whisper_model: Some("small".into()),
                ..RunOverrides::default()
            },
            &EnvOptions::default(),
            &Config::default(),
            &available(true, true),
        )
        .unwrap();
        assert!(matches!(resolved.engine, ResolvedEngine::Whisper { .. }));
    }

    #[test]
    fn parakeet_constraints_follow_source_precedence() {
        let file = Config {
            language: Some(german()),
            ..Config::default()
        };
        let resolved = resolve_run(
            &RunOverrides {
                engine: Some(EngineChoice::Parakeet),
                ..RunOverrides::default()
            },
            &EnvOptions::default(),
            &file,
            &available(true, true),
        )
        .unwrap();
        assert_eq!(resolved.engine, ResolvedEngine::ParakeetTdt06bV3);
        assert_eq!(resolved.language, LanguageChoice::Auto);

        let error = resolve_run(
            &RunOverrides {
                engine: Some(EngineChoice::Parakeet),
                language: Some(german()),
                ..RunOverrides::default()
            },
            &EnvOptions::default(),
            &file,
            &available(true, true),
        )
        .unwrap_err();
        assert!(matches!(error, PrepareError::InvalidRequest(_)));

        let configured = Config {
            engine: Some(EngineChoice::Parakeet),
            language: Some(german()),
            ..Config::default()
        };
        let resolved = resolve_run(
            &RunOverrides::default(),
            &EnvOptions::default(),
            &configured,
            &available(true, true),
        )
        .unwrap();
        assert_eq!(resolved.engine, ResolvedEngine::ParakeetTdt06bV3);
        assert_eq!(resolved.language, LanguageChoice::Auto);

        let resolved = resolve_run(
            &RunOverrides {
                language: Some(german()),
                ..RunOverrides::default()
            },
            &EnvOptions {
                engine: Some(EngineChoice::Parakeet),
                ..EnvOptions::default()
            },
            &Config::default(),
            &available(true, true),
        )
        .unwrap();
        assert!(matches!(resolved.engine, ResolvedEngine::Whisper { .. }));

        let error = resolve_run(
            &RunOverrides::default(),
            &EnvOptions {
                engine: Some(EngineChoice::Parakeet),
                language: Some(german()),
                ..EnvOptions::default()
            },
            &Config::default(),
            &available(true, true),
        )
        .unwrap_err();
        assert!(matches!(error, PrepareError::Configuration(_)));
    }

    #[test]
    fn auto_respects_models_from_every_configuration_source() {
        for (env, file) in [
            (
                EnvOptions {
                    whisper_model: Some("small".to_string()),
                    ..EnvOptions::default()
                },
                Config::default(),
            ),
            (
                EnvOptions::default(),
                Config {
                    whisper_model: Some("small".to_string()),
                    ..Config::default()
                },
            ),
        ] {
            let resolved = resolve_run(
                &RunOverrides::default(),
                &env,
                &file,
                &available(true, true),
            )
            .unwrap();
            assert!(matches!(resolved.engine, ResolvedEngine::Whisper { .. }));
        }
    }

    #[test]
    fn unconstrained_auto_prefers_parakeet_and_fake_is_explicit() {
        let resolved = resolve_run(
            &RunOverrides::default(),
            &EnvOptions::default(),
            &Config::default(),
            &available(true, true),
        )
        .unwrap();
        assert_eq!(resolved.engine, ResolvedEngine::ParakeetTdt06bV3);
        assert_eq!(resolved.language, LanguageChoice::Auto);

        let resolved = resolve_run(
            &RunOverrides::default(),
            &EnvOptions {
                engine: Some(EngineChoice::Fake),
                ..EnvOptions::default()
            },
            &Config::default(),
            &EngineAvailabilitySnapshot::default(),
        )
        .unwrap();
        assert_eq!(resolved.engine, ResolvedEngine::Fake);
        assert_eq!(resolved.language, LanguageChoice::default());
    }

    #[test]
    fn source_precedence_and_english_only_validation_are_explicit() {
        let file = Config {
            engine: Some(EngineChoice::Parakeet),
            language: Some(german()),
            cleanup: Some(CleanupMode::Off),
            ..Config::default()
        };
        let env = EnvOptions {
            engine: Some(EngineChoice::Whisper),
            language: Some(LanguageChoice::Auto),
            cleanup: Some(CleanupMode::Rules),
            ..EnvOptions::default()
        };
        let error = resolve_run(
            &RunOverrides::default(),
            &env,
            &file,
            &available(false, true),
        )
        .unwrap_err();
        assert!(matches!(error, PrepareError::Configuration(_)));

        let resolved = resolve_run(
            &RunOverrides {
                language: Some(LanguageChoice::Pinned(Language::ENGLISH)),
                ..RunOverrides::default()
            },
            &env,
            &file,
            &available(false, true),
        )
        .unwrap();
        assert!(matches!(resolved.engine, ResolvedEngine::Whisper { .. }));
        assert_eq!(resolved.cleanup, CleanupMode::Rules);
    }

    #[test]
    fn missing_setup_is_not_an_argument_error() {
        let error = resolve_run(
            &RunOverrides::default(),
            &EnvOptions::default(),
            &Config::default(),
            &EngineAvailabilitySnapshot::default(),
        )
        .unwrap_err();
        assert!(matches!(error, PrepareError::EngineMissing(_)));
    }

    #[test]
    fn catalog_projection_matches_auto_constraints() {
        let pinned = RequestedRun::new(
            &RunOverrides::default(),
            &EnvOptions::default(),
            &Config {
                language: Some(german()),
                ..Config::default()
            },
        );
        assert!(!projects_parakeet(&pinned, true));

        let unconstrained = RequestedRun::new(
            &RunOverrides::default(),
            &EnvOptions::default(),
            &Config::default(),
        );
        assert!(projects_parakeet(&unconstrained, true));
    }
}
