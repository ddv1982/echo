use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use echo_core::{
    Config, DecodeOptions, Dictionary, Engine, EngineChoice, EngineError, EngineId, Language,
    LanguageChoice, Pcm16kMono, RecognitionHints, RunDetail, WhisperAccelerationPreference,
};

use crate::install::ManagedPath;
use crate::stt::{
    accelerated_engine, preferred_runtime, resolved_whisper_acceleration, whisper_runtime_launch,
    FakeEngine, ModelCache, ParakeetEngine, SpeechRuntimeInventory, WhisperEngine,
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
    pub fn for_process(env: &EnvOptions, file: &Config, runtime: &SpeechRuntimeInventory) -> Self {
        let candidates = [env.whisper_model.clone(), file.whisper_model.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        Self::from_process(&runtime.cache, runtime, &candidates)
    }

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
        for (index, name) in model_candidates.iter().enumerate() {
            if model_candidates[..index].contains(name)
                || whisper_models.iter().any(|model| model.name == *name)
            {
                continue;
            }
            if let Some((path, multilingual)) = WhisperEngine::configured(cache.clone(), name)
                .selected_model_from_inventory(&runtime.external_models)
            {
                whisper_models.push(WhisperModelCapability {
                    name: name.clone(),
                    multilingual,
                    path: Some(path),
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

pub enum TranscriptionPurpose<'a> {
    Dictation(&'a Dictionary),
    DictionaryTraining,
}

impl PreparedTranscription {
    #[must_use]
    pub fn resolved(&self) -> &ResolvedRun {
        &self.resolved
    }

    pub fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        purpose: TranscriptionPurpose<'_>,
    ) -> Result<CompletedTranscription, TranscriptionError> {
        self.transcribe_bounded(
            pcm,
            purpose,
            Instant::now() + Duration::from_secs(15 * 60),
            &|| false,
        )
    }

    /// Runs a prepared engine under one deadline and cancellation signal.
    /// Recovery attempts inherit the same bound rather than starting a fresh
    /// timeout after an accelerated attempt fails.
    pub fn transcribe_bounded(
        &self,
        pcm: &Pcm16kMono,
        purpose: TranscriptionPurpose<'_>,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CompletedTranscription, TranscriptionError> {
        let hints = match (&self.resolved.engine, &purpose) {
            (ResolvedEngine::Whisper { .. }, TranscriptionPurpose::Dictation(dictionary)) => {
                RecognitionHints::from_dictionary(dictionary)
            }
            (
                ResolvedEngine::Whisper { .. }
                | ResolvedEngine::ParakeetTdt06bV3
                | ResolvedEngine::Fake,
                TranscriptionPurpose::DictionaryTraining,
            )
            | (
                ResolvedEngine::ParakeetTdt06bV3 | ResolvedEngine::Fake,
                TranscriptionPurpose::Dictation(_),
            ) => RecognitionHints::default(),
        };
        let hint_count = hints.terms().len();
        let options = DecodeOptions {
            language: self.resolved.language,
            hints,
        };
        let transcript = self
            .engine
            .transcribe_bounded(pcm, &options, deadline, cancelled)
            .map_err(TranscriptionError::Engine)?;
        let text = match purpose {
            TranscriptionPurpose::Dictation(dictionary) => dictionary.rewrite(&transcript.raw),
            TranscriptionPurpose::DictionaryTraining => transcript.raw.clone(),
        };
        Ok(CompletedTranscription {
            raw: transcript.raw,
            text,
            engine: transcript.engine,
            audio_ms: transcript.audio_ms,
            infer_ms: transcript.infer_ms,
            detail: transcript.detail,
            requested_language: self.resolved.language,
            hint_count,
        })
    }
}

pub fn prepare_with_config(
    overrides: RunOverrides,
    file: &Config,
) -> Result<PreparedTranscription, PrepareError> {
    let env = EnvOptions::read();
    let cache = ModelCache::from_env();
    let runtime = SpeechRuntimeInventory::from_cache(&cache);
    let resolved = resolve_with_process_inventory(&overrides, &env, file, &cache, &runtime)?;
    prepare_resolved(overrides, file, resolved, &runtime)
}

pub fn resolve_next_run_for_process(file: &Config) -> Result<ResolvedRun, PrepareError> {
    let overrides = RunOverrides::default();
    let env = EnvOptions::read();
    let cache = ModelCache::from_env();
    let runtime = SpeechRuntimeInventory::from_cache(&cache);
    resolve_with_process_inventory(&overrides, &env, file, &cache, &runtime)
}

fn resolve_with_process_inventory(
    overrides: &RunOverrides,
    env: &EnvOptions,
    file: &Config,
    cache: &ModelCache,
    runtime: &SpeechRuntimeInventory,
) -> Result<ResolvedRun, PrepareError> {
    let model_candidates = [
        overrides.whisper_model.clone(),
        env.whisper_model.clone(),
        file.whisper_model.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let available = EngineAvailabilitySnapshot::from_process(cache, runtime, &model_candidates);
    resolve_run(overrides, env, file, &available)
}

pub fn prepare_resolved(
    overrides: RunOverrides,
    file: &Config,
    resolved: ResolvedRun,
    runtime: &SpeechRuntimeInventory,
) -> Result<PreparedTranscription, PrepareError> {
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
            let preference = resolved_whisper_acceleration(
                overrides.whisper_acceleration,
                file.whisper_acceleration,
            );
            // An explicit tuning, driver, or cache override means the caller is
            // driving the runtime themselves, so the GPU contract does not apply.
            let wants_gpu = preference == WhisperAccelerationPreference::Gpu
                && overrides.whisper_tuning.is_none()
                && !overrides.whisper_force_cpu
                && overrides.whisper_vulkan_driver_files.is_none()
                && overrides.whisper_mesa_shader_cache_dir.is_none();
            // Selected last so the leased path pops off the end, and only when
            // this run may actually use it. lock_selected fails the whole call
            // if any named component cannot be leased, so leasing the GPU
            // runtime for a CPU run would let removing it break dictation that
            // never touches it. Taken from the inventory snapshot rather than a
            // second lookup, because a path this snapshot never saw is passed
            // through unleased instead of failing.
            let gpu_cli = wants_gpu.then_some(()).and(runtime.vulkan_cli.clone());
            let mut selected = vec![runtime_candidate.cli.clone()];
            if let Some(server) = &runtime_candidate.server {
                selected.push(server.clone());
            }
            selected.push(model_path.clone());
            if let Some(vad) = &vad {
                selected.push(vad.clone());
            }
            if let Some(gpu_cli) = &gpu_cli {
                selected.push(gpu_cli.clone());
            }
            let locked = runtime
                .lock_selected(&selected)
                .map_err(PrepareError::EngineMissing)?;
            let mut leased_gpu_runtime = None;
            let mut locked_paths = locked.paths;
            if gpu_cli.is_some() {
                leased_gpu_runtime = locked_paths
                    .pop()
                    .and_then(|cli| cli.parent().map(std::path::Path::to_path_buf));
            }
            let mut paths = locked_paths.into_iter();
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
            plan.force_cpu = overrides.whisper_force_cpu
                || preference == WhisperAccelerationPreference::Cpu
                || (plan.runtime.source == echo_core::WhisperRuntimeSource::Managed
                    && plan.runtime.backend == echo_core::WhisperRuntimeBackend::Cpu);
            let engine: Box<dyn Engine> = if wants_gpu {
                let accelerated = leased_gpu_runtime
                    .as_deref()
                    .ok_or(echo_core::WhisperAccelerationSkip::RuntimeMissing)
                    .and_then(|runtime| {
                        accelerated_engine(runtime, &plan, file.whisper_gpu_device.as_deref())
                    });
                match accelerated {
                    Ok(engine) => Box::new(engine),
                    Err(reason) => {
                        // A run the gate refused is a CPU run by construction,
                        // not by luck of which binary preferred_runtime picked.
                        // force_cpu is false for a system whisper-cli, and
                        // distributions ship Vulkan-capable builds of it, so
                        // without this a refusal would hand the run to a GPU
                        // with no pin, no receipt check, and no quarantine.
                        plan.force_cpu = true;
                        Box::new(WhisperEngine::with_plan(plan).skipped_acceleration(reason))
                    }
                }
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
) -> Result<CompletedTranscription, TranscriptionError> {
    let capture = crate::audio::load_wav(path).map_err(TranscriptionError::Audio)?;
    prepared.transcribe(&capture.pcm, TranscriptionPurpose::Dictation(dictionary))
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
}

impl std::fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(error) => error.fmt(f),
            Self::Engine(error) => error.fmt(f),
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
    let available = EngineAvailabilitySnapshot::for_process(&env, file, &runtime);
    language_catalog_from_available(engine, &env, file, &available)
}

pub fn language_catalog_from_available(
    engine: Option<EngineChoice>,
    env: &EnvOptions,
    file: &Config,
    available: &EngineAvailabilitySnapshot,
) -> LanguageCatalog {
    let requested = RequestedRun::new(
        &RunOverrides {
            engine,
            ..RunOverrides::default()
        },
        env,
        file,
    );
    let parakeet = projects_parakeet(&requested, available.parakeet);
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
    let model = requested
        .whisper_model
        .or_else(|| available.best_whisper().map(|model| model.name.clone()));
    let multilingual = model.as_deref().and_then(|name| {
        available
            .whisper_models
            .iter()
            .find(|model| model.name == name)
            .map(|model| model.multilingual)
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
    use std::sync::{Arc, Mutex};

    struct InspectingEngine {
        hints: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl Engine for InspectingEngine {
        fn id(&self) -> EngineId {
            EngineId::Whisper {
                model: "test".to_string(),
            }
        }

        fn transcribe_bounded(
            &self,
            pcm: &Pcm16kMono,
            options: &DecodeOptions,
            deadline: Instant,
            cancelled: &dyn Fn() -> bool,
        ) -> Result<echo_core::Transcript, EngineError> {
            if cancelled() {
                return Err(EngineError::Infer("transcription canceled".to_string()));
            }
            if Instant::now() >= deadline {
                return Err(EngineError::Infer(
                    "transcription deadline expired".to_string(),
                ));
            }
            self.hints
                .lock()
                .expect("hint capture lock")
                .push(options.hints.terms().to_vec());
            Ok(echo_core::Transcript {
                raw: "clawed code".to_string(),
                engine: self.id(),
                audio_ms: pcm.duration_ms(),
                infer_ms: 1,
                detail: RunDetail::default(),
            })
        }
    }

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
    fn dictionary_training_returns_raw_output_without_dictionary_hints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictionary.json");
        let mut dictionary = Dictionary::load_from(&path).unwrap();
        dictionary.add("clawed code", "Claude Code").unwrap();
        let hints = Arc::new(Mutex::new(Vec::new()));
        let prepared = PreparedTranscription {
            resolved: ResolvedRun {
                engine: ResolvedEngine::Whisper {
                    model: "test".to_string(),
                    multilingual: true,
                    model_path: None,
                },
                language: LanguageChoice::Auto,
            },
            engine: Box::new(InspectingEngine {
                hints: Arc::clone(&hints),
            }),
            _managed_paths: Vec::new(),
        };
        let pcm = Pcm16kMono::from_samples(vec![8_000; 160]);

        let dictation = prepared
            .transcribe(&pcm, TranscriptionPurpose::Dictation(&dictionary))
            .unwrap();
        let training = prepared
            .transcribe(&pcm, TranscriptionPurpose::DictionaryTraining)
            .unwrap();

        assert_eq!(dictation.text, "Claude Code");
        assert_eq!(dictation.hint_count, 1);
        assert_eq!(training.raw, "clawed code");
        assert_eq!(training.text, "clawed code");
        assert_eq!(training.hint_count, 0);
        assert_eq!(
            *hints.lock().expect("hint capture lock"),
            vec![vec!["Claude Code".to_string()], Vec::<String>::new()]
        );
    }

    #[test]
    fn prepared_transcription_forwards_cancellation_before_engine_work() {
        let hints = Arc::new(Mutex::new(Vec::new()));
        let prepared = PreparedTranscription {
            resolved: ResolvedRun {
                engine: ResolvedEngine::Fake,
                language: LanguageChoice::Auto,
            },
            engine: Box::new(InspectingEngine {
                hints: Arc::clone(&hints),
            }),
            _managed_paths: Vec::new(),
        };
        let error = prepared
            .transcribe_bounded(
                &Pcm16kMono::from_samples(vec![8_000; 160]),
                TranscriptionPurpose::DictionaryTraining,
                Instant::now() + Duration::from_secs(1),
                &|| true,
            )
            .unwrap_err();
        assert!(error.to_string().contains("canceled"));
        assert!(hints.lock().expect("hint capture lock").is_empty());
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
            ..Config::default()
        };
        let env = EnvOptions {
            engine: Some(EngineChoice::Whisper),
            language: Some(LanguageChoice::Auto),
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
    #[test]
    fn presentation_catalog_preserves_requested_model_and_unavailable_engine_semantics() {
        let env = EnvOptions::default();
        let absent = EngineAvailabilitySnapshot::default();
        let defaults = language_catalog_from_available(None, &env, &Config::default(), &absent);
        assert_eq!(defaults.selection, LanguageSelection::AutoOrPinned);
        assert_eq!(defaults.model, None);
        let pinned = Config {
            whisper_model: Some("missing.en".into()),
            ..Config::default()
        };
        let missing = language_catalog_from_available(None, &env, &pinned, &absent);
        assert_eq!(missing.model.as_deref(), Some("missing.en"));
        assert_eq!(missing.selection, LanguageSelection::AutoOrPinned);
        let parakeet = Config {
            engine: Some(EngineChoice::Parakeet),
            ..Config::default()
        };
        let catalog = language_catalog_from_available(None, &env, &parakeet, &absent);
        assert_eq!(catalog.selection, LanguageSelection::AutomaticOnly);
        assert!(resolve_run(&RunOverrides::default(), &env, &parakeet, &absent).is_err());
    }

    #[test]
    fn presentation_resolves_custom_configured_model_from_shared_inventory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("personal.bin");
        std::fs::write(&path, []).unwrap();
        let runtime = SpeechRuntimeInventory::from_cache(&ModelCache::at(root.path()));
        assert!(runtime.models.whisper.is_empty());
        let file = Config {
            whisper_model: Some("missing".into()),
            ..Config::default()
        };
        let env = EnvOptions {
            whisper_model: Some("personal".into()),
            ..EnvOptions::default()
        };
        let mut available = EngineAvailabilitySnapshot::for_process(&env, &file, &runtime);
        available.whisper_binary = true;
        let catalog = language_catalog_from_available(None, &env, &file, &available);
        assert_eq!(catalog.model.as_deref(), Some("personal"));
        assert_eq!(catalog.selection, LanguageSelection::AutoOrPinned);
        let run = resolve_run(&RunOverrides::default(), &env, &file, &available).unwrap();
        assert!(
            matches!(run.engine, ResolvedEngine::Whisper { model_path: Some(model), .. } if model == path)
        );
        std::fs::remove_file(&path).unwrap();
        let refreshed = SpeechRuntimeInventory::from_cache(&ModelCache::at(root.path()));
        let available = EngineAvailabilitySnapshot::for_process(&env, &file, &refreshed);
        assert!(available.whisper_models.is_empty());
    }
}
