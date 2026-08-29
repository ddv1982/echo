use std::fs;
use std::num::NonZeroUsize;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use echo_core::{
    strip_nonspeech, DecodeOptions, Engine, EngineError, EngineId, Language, LanguageChoice,
    Pcm16kMono, RunDetail, Transcript, WhisperAttemptTelemetry, WhisperRetryReason, WhisperRunMode,
    WhisperRunTelemetry, WhisperRuntimeBackend, WhisperRuntimeSource, WhisperRuntimeTelemetry,
    WhisperTuningTelemetry,
};
use serde::Deserialize;

use super::cache::{parse_whisper_filename, ModelCache};
use super::whisper_behavior::{
    CHILD_REAP_TIMEOUT_SECS, CLEARED_ENVIRONMENT_KEYS, CLEARED_ENVIRONMENT_PREFIXES,
    ONE_SHOT_TIMEOUT_SECS,
};
use super::whisper_probe::{
    observe_runtime, parse_vulkan_devices, parse_vulkan_runtime_receipt,
    parse_vulkan_runtime_receipt_line,
};
use super::whisper_runtime_launch;
use super::write_temp_wav;
use super::{
    WhisperExecutionPlan, WhisperModelAsset, WhisperProtocol, WhisperRuntimeCandidate,
    WhisperRuntimeLaunch, WhisperTuning,
};
use crate::which::path_of;

pub struct WhisperEngine {
    model: String,
    files: WhisperFiles,
}

enum WhisperFiles {
    Discover(ModelCache),
    Explicit(Box<WhisperExecutionPlan>),
}

impl WhisperEngine {
    #[must_use]
    pub fn configured(cache: ModelCache, model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            files: WhisperFiles::Discover(cache),
        }
    }

    #[must_use]
    pub fn with_plan(plan: WhisperExecutionPlan) -> Self {
        Self {
            model: plan.model.name.clone(),
            files: WhisperFiles::Explicit(Box::new(plan)),
        }
    }

    #[must_use]
    pub fn with_paths(
        model_name: impl Into<String>,
        binary: PathBuf,
        model: PathBuf,
        vad: Option<PathBuf>,
        multilingual: bool,
    ) -> Self {
        let model_name = model_name.into();
        let launch = whisper_runtime_launch(&binary);
        Self::with_plan(WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source: WhisperRuntimeSource::Unknown,
                backend: WhisperRuntimeBackend::Unknown,
                cli: binary,
                server: None,
                launch,
            },
            WhisperModelAsset {
                name: model_name,
                path: model,
                multilingual,
            },
            vad,
        ))
    }

    /// True when both the runner binary and a model file are installed.
    /// A metadata check only; no inference runs.
    #[must_use]
    pub fn available(&self) -> bool {
        self.model_file().is_some() && self.resolved_binary().is_some()
    }

    #[must_use]
    pub fn model_name(&self) -> Option<&str> {
        Some(&self.model)
    }

    fn model_file(&self) -> Option<PathBuf> {
        self.selected_model().map(|(path, _)| path)
    }

    /// The model file plus its filename-derived multilingual flag. The flag
    /// is a pre-flight guess used to refuse impossible language choices; the
    /// authoritative value is `model.multilingual` in the engine's JSON.
    pub(crate) fn selected_model(&self) -> Option<(PathBuf, bool)> {
        if let WhisperFiles::Explicit(plan) = &self.files {
            return Some((plan.model.path.clone(), plan.model.multilingual));
        }
        let WhisperFiles::Discover(cache) = &self.files else {
            return None;
        };
        let model = self.model.as_str();
        let inventory = cache.inventory();
        if let Some(installed) = inventory.whisper.iter().find(|m| m.name == model) {
            return Some((installed.path.clone(), installed.multilingual));
        }
        // A configured name outside the GGML convention still resolves, so a
        // fine-tuned file the scanner ignores remains usable when pinned.
        let candidates = [
            format!("ggml-{model}.bin"),
            format!("{model}.bin"),
            format!("ggml-{model}.gguf"),
        ];
        candidates
            .into_iter()
            .map(|name| cache.path(&name))
            .find(|path| path.is_file())
            .map(|path| {
                let multilingual = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(parse_whisper_filename)
                    .map(|(_, _, multilingual, _)| multilingual)
                    .unwrap_or(true);
                (path, multilingual)
            })
    }

    pub(crate) fn binary() -> Option<PathBuf> {
        ["whisper-cli", "whisper-cpp", "whisper"]
            .into_iter()
            .find_map(path_of)
    }

    fn resolved_binary(&self) -> Option<PathBuf> {
        match &self.files {
            WhisperFiles::Explicit(plan) => Some(plan.runtime.cli.clone()),
            WhisperFiles::Discover(_) => Self::binary(),
        }
    }

    fn vad_model(&self) -> Option<PathBuf> {
        match &self.files {
            WhisperFiles::Explicit(plan) => plan.vad.clone(),
            WhisperFiles::Discover(cache) => cache.vad_model(),
        }
    }

    fn tuning(&self) -> WhisperTuning {
        match &self.files {
            WhisperFiles::Explicit(plan) => plan.tuning,
            WhisperFiles::Discover(_) => WhisperTuning::runtime_defaults(),
        }
    }

    fn runtime_identity(
        &self,
        binary: String,
        stderr: &str,
        launch: Option<&WhisperRuntimeLaunch>,
    ) -> WhisperRuntimeTelemetry {
        let mut runtime = match &self.files {
            WhisperFiles::Explicit(plan) => {
                let launch = launch.expect("explicit Whisper plans have a launch contract");
                WhisperRuntimeTelemetry {
                    binary,
                    source: plan.runtime.source,
                    backend: plan.runtime.backend,
                    device: None,
                    library_path: launch
                        .library_dir
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    vulkan_driver_files: launch
                        .vulkan_driver_files
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    mesa_shader_cache_dir: launch
                        .mesa_shader_cache_dir
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    identity_sha256: launch.identity_sha256.clone(),
                    vulkan_receipt: None,
                }
            }
            WhisperFiles::Discover(_) => WhisperRuntimeTelemetry {
                binary,
                source: WhisperRuntimeSource::System,
                backend: WhisperRuntimeBackend::Unknown,
                device: None,
                library_path: None,
                vulkan_driver_files: None,
                mesa_shader_cache_dir: None,
                identity_sha256: None,
                vulkan_receipt: None,
            },
        };
        if let Some(observed) = observe_runtime(stderr) {
            runtime.backend = observed.backend;
            runtime.device = observed.device;
        }
        if runtime.backend == WhisperRuntimeBackend::Vulkan {
            runtime.vulkan_receipt = parse_vulkan_runtime_receipt(stderr).ok();
        }
        runtime
    }

    fn protocol(&self) -> WhisperProtocol {
        match &self.files {
            WhisperFiles::Explicit(plan) => plan.protocol,
            WhisperFiles::Discover(_) => WhisperProtocol::OneShotCli,
        }
    }

    fn force_cpu(&self) -> bool {
        matches!(&self.files, WhisperFiles::Explicit(plan) if plan.force_cpu)
            || (cfg!(debug_assertions)
                && matches!(
                    &self.files,
                    WhisperFiles::Explicit(plan)
                        if plan.runtime.backend == WhisperRuntimeBackend::Vulkan
                )
                && std::env::var("ECHO_WHISPER_TEST_FAULT").as_deref() == Ok("backend-fallback"))
    }

    fn timeout(&self) -> Duration {
        if cfg!(debug_assertions)
            && matches!(
                &self.files,
                WhisperFiles::Explicit(plan)
                    if plan.runtime.backend == WhisperRuntimeBackend::Vulkan
            )
            && std::env::var("ECHO_WHISPER_TEST_FAULT").as_deref() == Ok("gpu-timeout")
        {
            return Duration::from_millis(1);
        }
        match &self.files {
            WhisperFiles::Explicit(plan) => plan.timeout,
            WhisperFiles::Discover(_) => Duration::from_secs(ONE_SHOT_TIMEOUT_SECS),
        }
    }

    fn allow_vad_retry(&self) -> bool {
        match &self.files {
            WhisperFiles::Explicit(plan) => plan.allow_vad_retry,
            WhisperFiles::Discover(_) => true,
        }
    }

    fn runtime_launch(&self) -> Option<&WhisperRuntimeLaunch> {
        match &self.files {
            WhisperFiles::Explicit(plan) => Some(&plan.runtime.launch),
            WhisperFiles::Discover(_) => None,
        }
    }

    fn effective_runtime_launch(&self, binary: &Path) -> Option<WhisperRuntimeLaunch> {
        let configured = self.runtime_launch()?;
        let mut effective = whisper_runtime_launch(binary);
        effective.vulkan_driver_files = configured.vulkan_driver_files.clone();
        effective.mesa_shader_cache_dir = configured.mesa_shader_cache_dir.clone();
        effective.vulkan_selector = configured.vulkan_selector.clone();
        effective.cancel_on_recording = configured.cancel_on_recording.clone();
        Some(effective)
    }
}

impl Engine for WhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.model.clone(),
        }
    }

    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError> {
        if !matches!(self.protocol(), WhisperProtocol::OneShotCli) {
            return Err(EngineError::Infer(
                "resident Whisper execution is not available".to_string(),
            ));
        }
        let (model, multilingual) = self.selected_model().ok_or(EngineError::Missing)?;
        refuse_impossible_language(&model, multilingual, options.language)?;
        let bin = self.resolved_binary().ok_or(EngineError::Missing)?;
        let launch = self.effective_runtime_launch(&bin);
        let started = Instant::now();
        let encode_started = Instant::now();
        let wav = TempWav::new(write_temp_wav(pcm).map_err(EngineError::Infer)?);
        let audio_encode_ms = elapsed_ms(encode_started);
        let vad = self.vad_model();
        let tuning = self.tuning();
        let timeout = self.timeout();
        let (first, mut first_telemetry) = run_attempt(
            &bin,
            launch.as_ref(),
            whisper_args_with_tuning(
                &model,
                wav.path(),
                vad.as_deref(),
                options,
                tuning,
                self.force_cpu(),
            ),
            vad.is_some(),
            timeout,
        )?;
        let retry_without_vad = self.allow_vad_retry()
            && !first.status.success()
            && vad.is_some()
            && should_retry_without_vad(&String::from_utf8_lossy(&first.stderr));
        let (status, vad_active, mode, attempts) = if retry_without_vad {
            first_telemetry.retry_reason = Some(WhisperRetryReason::VadRejected);
            let (retry, retry_telemetry) = run_attempt(
                &bin,
                launch.as_ref(),
                whisper_args_with_tuning(
                    &model,
                    wav.path(),
                    None,
                    options,
                    tuning,
                    self.force_cpu(),
                ),
                false,
                timeout,
            )?;
            (
                retry,
                false,
                WhisperRunMode::ColdFallback,
                vec![first_telemetry, retry_telemetry],
            )
        } else {
            (
                first,
                vad.is_some(),
                WhisperRunMode::ColdCli,
                vec![first_telemetry],
            )
        };
        let _ = fs::remove_file(wav.path());
        let parse_started = Instant::now();
        let stderr = String::from_utf8_lossy(&status.stderr);
        let parsed = finish_whisper(status.status.success(), &status.stdout, &stderr)?;
        let parse_ms = elapsed_ms(parse_started);
        let total_ms = elapsed_ms(started);
        let binary = bin.to_string_lossy().into_owned();
        Ok(Transcript {
            raw: raw_text(&parsed.text),
            engine: EngineId::Whisper {
                model: parsed.model,
            },
            audio_ms: pcm.duration_ms(),
            infer_ms: total_ms,
            detail: RunDetail {
                binary: Some(binary.clone()),
                model_path: Some(model.to_string_lossy().into_owned()),
                vad_path: vad.as_ref().map(|path| path.to_string_lossy().into_owned()),
                multilingual: Some(parsed.multilingual),
                vad: Some(vad_active),
                language: parsed.language.clone(),
                language_probability: parsed.language_probability,
                whisper: Some(WhisperRunTelemetry {
                    mode,
                    total_ms,
                    audio_encode_ms,
                    parse_ms,
                    runtime: self.runtime_identity(binary, &stderr, launch.as_ref()),
                    tuning: WhisperTuningTelemetry {
                        threads: tuning.threads.map(NonZeroUsize::get),
                        beam_size: tuning.beam_size,
                        best_of: tuning.best_of,
                        no_fallback: tuning.no_fallback,
                    },
                    attempts,
                    recovery: None,
                    selection: None,
                }),
            },
        })
    }
}

struct TempWav(PathBuf);

impl TempWav {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn run_attempt(
    binary: &Path,
    launch: Option<&WhisperRuntimeLaunch>,
    args: Vec<String>,
    vad: bool,
    timeout: Duration,
) -> Result<(Output, WhisperAttemptTelemetry), EngineError> {
    let wall_started = Instant::now();
    let spawn_started = Instant::now();
    let mut command = command_for_runtime(binary, launch);
    command.process_group(0);
    let child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| EngineError::Infer(error.to_string()))?;
    let process_start_ms = elapsed_ms(spawn_started);
    let pid =
        rustix::process::Pid::from_raw(i32::try_from(child.id()).map_err(|_| {
            EngineError::Infer("Whisper child process ID is out of range".to_string())
        })?)
        .ok_or_else(|| EngineError::Infer("Whisper child process ID is zero".to_string()))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let deadline = Instant::now() + timeout;
    let recording_lock = launch.and_then(|launch| launch.cancel_on_recording.as_deref());
    let output = loop {
        if recording_lock.is_some_and(crate::rec::session_active_at) {
            kill_and_reap(pid, &receiver);
            return Err(EngineError::Infer(
                "Whisper calibration canceled because recording started".to_string(),
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            kill_and_reap(pid, &receiver);
            return Err(EngineError::Infer(format!(
                "Whisper runtime timed out after {} ms",
                timeout.as_millis()
            )));
        }
        let wait = (deadline - now).min(Duration::from_millis(50));
        match receiver.recv_timeout(wait) {
            Ok(output) => {
                break output.map_err(|error| EngineError::Infer(error.to_string()))?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(EngineError::Infer(
                    "Whisper runtime wait thread stopped unexpectedly".to_string(),
                ));
            }
        }
    };
    let telemetry = WhisperAttemptTelemetry {
        vad,
        process_start_ms,
        child_wall_ms: elapsed_ms(wall_started),
        success: output.status.success(),
        exit_code: output.status.code(),
        retry_reason: None,
    };
    Ok((output, telemetry))
}

fn kill_and_reap(pid: rustix::process::Pid, receiver: &mpsc::Receiver<std::io::Result<Output>>) {
    if rustix::process::kill_process_group(pid, rustix::process::Signal::KILL).is_err() {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
    let _ = receiver.recv_timeout(Duration::from_secs(CHILD_REAP_TIMEOUT_SECS));
}

pub(crate) fn probe_vulkan_runtime_receipt(
    binary: &Path,
    launch: &WhisperRuntimeLaunch,
    timeout: Duration,
) -> Result<echo_core::WhisperVulkanReceipt, String> {
    let (output, _) = run_attempt(
        binary,
        Some(launch),
        vec!["--ready-vulkan".to_string()],
        false,
        timeout,
    )
    .map_err(|error| error.to_string())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(stderr.into_owned());
    }
    parse_vulkan_runtime_receipt_line(&stderr)
}

pub(crate) fn enumerate_vulkan_runtime_receipts(
    binary: &Path,
    launch: &WhisperRuntimeLaunch,
    timeout: Duration,
) -> Result<(Vec<echo_core::WhisperVulkanReceipt>, String), String> {
    let (output, _) = run_attempt(
        binary,
        Some(launch),
        vec!["--list-vulkan-json".to_string()],
        false,
        timeout,
    )
    .map_err(|error| error.to_string())?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(stderr);
    }
    // Receipts arrive on stdout; the human-readable device names the picker
    // shows are only ever printed to stderr.
    Ok((
        parse_vulkan_devices(&String::from_utf8_lossy(&output.stdout))?,
        stderr,
    ))
}

fn command_for_runtime(binary: &Path, launch: Option<&WhisperRuntimeLaunch>) -> Command {
    let mut command = Command::new(binary);
    let Some(launch) = launch else {
        return command;
    };
    for name in CLEARED_ENVIRONMENT_KEYS {
        command.env_remove(name);
    }
    for name in [
        "ECHO_WHISPER_VULKAN_DEVICE_UUID",
        "ECHO_WHISPER_VULKAN_DRIVER_UUID",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if is_inference_environment_selector(&name) {
            command.env_remove(name);
        }
    }
    if let Some(path) = &launch.library_dir {
        command.env("LD_LIBRARY_PATH", path);
    }
    if let Some(path) = &launch.vulkan_driver_files {
        command.env("VK_DRIVER_FILES", path);
    }
    if let Some(path) = &launch.mesa_shader_cache_dir {
        command.env("MESA_SHADER_CACHE_DIR", path);
    }
    if let Some(selector) = &launch.vulkan_selector {
        command.env("ECHO_WHISPER_VULKAN_DEVICE_UUID", selector.device_uuid());
        command.env("ECHO_WHISPER_VULKAN_DRIVER_UUID", selector.driver_uuid());
    }
    command
}

fn is_inference_environment_selector(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    CLEARED_ENVIRONMENT_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn should_retry_without_vad(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        let runtime_failure = line.contains("failed to")
            && (line.contains("vad context")
                || line.contains("vad model")
                || line.contains("compute vad"));
        let unsupported_flag = ["unknown", "unrecognized", "unsupported", "invalid"]
            .iter()
            .any(|word| line.contains(word))
            && ["--vad", "--vad-model", "-vm"]
                .iter()
                .any(|flag| line.contains(flag));
        runtime_failure || unsupported_flag
    })
}

/// Refuse before spawning when the model cannot honour the language choice.
/// Measured upstream: an `.en` model given `-l de` prints a warning, resets
/// to English, transcribes English, and exits 0, so passing the flag through
/// would return confident English text for German speech. `-dl` bypasses that
/// guard through an upstream bug and is never invoked.
fn refuse_impossible_language(
    model: &Path,
    multilingual: bool,
    choice: LanguageChoice,
) -> Result<(), EngineError> {
    if multilingual {
        return Ok(());
    }
    let wants = match choice {
        LanguageChoice::Pinned(Language::ENGLISH) => return Ok(()),
        LanguageChoice::Pinned(language) => language.english_name().to_string(),
        LanguageChoice::Auto => "automatic language detection".to_string(),
    };
    let name = model
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "The selected model".to_string());
    Err(EngineError::Infer(format!(
        "{name} is an English-only model and cannot do {wants}. \
         Choose a multilingual model or set the language to English."
    )))
}

#[cfg(test)]
fn whisper_args(
    model: &Path,
    wav: &Path,
    vad: Option<&Path>,
    options: &DecodeOptions,
) -> Vec<String> {
    whisper_args_with_tuning(
        model,
        wav,
        vad,
        options,
        WhisperTuning::runtime_defaults(),
        false,
    )
}

fn whisper_args_with_tuning(
    model: &Path,
    wav: &Path,
    vad: Option<&Path>,
    options: &DecodeOptions,
    tuning: WhisperTuning,
    force_cpu: bool,
) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-nt".into(),
        "-oj".into(),
        "-of".into(),
        "-".into(),
        "-l".into(),
        match options.language {
            LanguageChoice::Auto => "auto".to_string(),
            LanguageChoice::Pinned(language) => language.code().to_string(),
        },
    ];
    if let Some(threads) = tuning.threads {
        args.extend(["-t".into(), threads.get().to_string()]);
    }
    if let Some(beam_size) = tuning.beam_size {
        args.extend(["-bs".into(), beam_size.to_string()]);
    }
    if let Some(best_of) = tuning.best_of {
        args.extend(["-bo".into(), best_of.to_string()]);
    }
    if tuning.no_fallback == Some(true) {
        args.push("-nf".into());
    }
    if force_cpu {
        args.push("--no-gpu".into());
    }
    if !options.hints.is_empty() {
        args.push("--prompt".into());
        args.push(options.hints.terms().join(", "));
    }
    if let Some(vad) = vad {
        args.push("--vad".into());
        args.push("-vm".into());
        args.push(vad.to_string_lossy().into_owned());
    }
    args
}

fn raw_text(text: &str) -> String {
    strip_nonspeech(text.trim()).to_string()
}

#[derive(Debug, Clone, PartialEq)]
struct WhisperParse {
    text: String,
    language: Option<String>,
    model: String,
    multilingual: bool,
    language_probability: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct WhisperOutput {
    model: ModelInfo,
    #[serde(default)]
    result: ResultInfo,
    #[serde(default)]
    transcription: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    #[serde(rename = "type")]
    model_type: String,
    multilingual: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ResultInfo {
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    text: String,
}

fn finish_whisper(success: bool, stdout: &[u8], stderr: &str) -> Result<WhisperParse, EngineError> {
    if !success {
        return Err(EngineError::Infer(stderr.to_string()));
    }
    let mut parsed = parse_whisper_stdout(stdout)?;
    parsed.language_probability = parse_detection_probability(stderr);
    Ok(parsed)
}

/// whisper.cpp prints `auto-detected language: de (p = 0.973123)` on stderr
/// when detection runs; the JSON carries only the code, no probability.
fn parse_detection_probability(stderr: &str) -> Option<f32> {
    let line = stderr
        .lines()
        .find(|line| line.contains("auto-detected language:"))?;
    let after = line.split("p = ").nth(1)?;
    let end = after.find(')')?;
    after[..end].trim().parse().ok()
}

fn parse_whisper_stdout(stdout: &[u8]) -> Result<WhisperParse, EngineError> {
    parse_whisper_json(&String::from_utf8_lossy(stdout))
}

fn parse_whisper_json(raw: &str) -> Result<WhisperParse, EngineError> {
    let output: WhisperOutput = serde_json::from_str(raw.trim())
        .map_err(|err| EngineError::Infer(format!("whisper json: {err}")))?;
    let text = output
        .transcription
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>()
        .trim()
        .to_string();
    let language = if output.transcription.is_empty() {
        None
    } else {
        output.result.language.filter(|code| !code.is_empty())
    };
    Ok(WhisperParse {
        text,
        language,
        model: output.model.model_type,
        multilingual: output.model.multilingual,
        language_probability: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::VulkanRuntimeSelector;
    use echo_core::{Dictionary, RecognitionHints};

    fn options(language: LanguageChoice) -> DecodeOptions {
        DecodeOptions {
            language,
            hints: RecognitionHints::default(),
        }
    }

    fn options_with_hint(language: LanguageChoice, written: &str) -> DecodeOptions {
        let dir = std::env::temp_dir().join(format!(
            "echo-whisper-hints-{}-{}",
            std::process::id(),
            written.len()
        ));
        let _ = fs::create_dir_all(&dir);
        let mut dictionary = Dictionary::load_from(dir.join("dictionary.json")).unwrap();
        dictionary.add("misheard", written).unwrap();
        DecodeOptions {
            language,
            hints: RecognitionHints::from_dictionary(&dictionary),
        }
    }

    #[test]
    fn missing_model_is_engine_missing() {
        let dir = std::env::temp_dir().join("echo-empty-whisper-models");
        let _ = fs::create_dir_all(&dir);
        let engine = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);
        assert_eq!(
            engine.transcribe(&pcm, &options(LanguageChoice::default())),
            Err(EngineError::Missing)
        );
    }

    #[test]
    fn blank_audio_raw_is_empty() {
        assert!(raw_text("[BLANK_AUDIO]").is_empty());
    }

    #[test]
    fn explicit_launch_contract_replaces_loader_and_cache_state() {
        let launch = WhisperRuntimeLaunch {
            library_dir: Some(PathBuf::from("/runtime")),
            vulkan_driver_files: Some(PathBuf::from("/driver.json")),
            mesa_shader_cache_dir: Some(PathBuf::from("/shader-cache")),
            vulkan_selector: Some(
                VulkanRuntimeSelector::parse("1".repeat(32), "2".repeat(32)).unwrap(),
            ),
            identity_sha256: Some("a".repeat(64)),
            cancel_on_recording: None,
        };
        let command = command_for_runtime(Path::new("whisper-cli"), Some(&launch));
        let value = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value)
        };
        assert_eq!(
            value("LD_LIBRARY_PATH").flatten(),
            Some(std::ffi::OsStr::new("/runtime"))
        );
        assert_eq!(
            value("VK_DRIVER_FILES").flatten(),
            Some(std::ffi::OsStr::new("/driver.json"))
        );
        assert_eq!(
            value("MESA_SHADER_CACHE_DIR").flatten(),
            Some(std::ffi::OsStr::new("/shader-cache"))
        );
        assert_eq!(
            value("ECHO_WHISPER_VULKAN_DEVICE_UUID").flatten(),
            Some(std::ffi::OsStr::new("11111111111111111111111111111111"))
        );
        assert_eq!(
            value("ECHO_WHISPER_VULKAN_DRIVER_UUID").flatten(),
            Some(std::ffi::OsStr::new("22222222222222222222222222222222"))
        );
        assert_eq!(value("LD_PRELOAD"), Some(None));
        assert_eq!(value("VK_ICD_FILENAMES"), Some(None));
        assert!(command_for_runtime(Path::new("whisper-cli"), None)
            .get_envs()
            .next()
            .is_none());
    }

    #[test]
    fn launch_contract_recognizes_loader_and_device_selector_namespaces() {
        for name in [
            "LD_DEBUG",
            "MESA_VK_DEVICE_SELECT",
            "DRI_PRIME",
            "VK_LOADER_LAYERS_ENABLE",
            "RADV_PERFTEST",
            "__GLX_VENDOR_LIBRARY_NAME",
            "CUDA_VISIBLE_DEVICES",
            "GGML_VK_VISIBLE_DEVICES",
            "HSA_OVERRIDE_GFX_VERSION",
        ] {
            assert!(
                is_inference_environment_selector(std::ffi::OsStr::new(name)),
                "{name}"
            );
        }
        for name in ["ECHO_MODEL_DIR", "LANG", "PATH"] {
            assert!(!is_inference_environment_selector(std::ffi::OsStr::new(
                name
            )));
        }
    }

    #[test]
    fn calibration_child_is_reaped_when_recording_starts() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "echo-whisper-calibration-cancel-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("whisper-cli");
        fs::write(&binary, "#!/bin/sh\nsleep 5\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let lock = root.join("recording.lock");
        fs::write(&lock, format!("{}\ntest-token\n", std::process::id())).unwrap();
        let mut launch = whisper_runtime_launch(&binary);
        launch.cancel_on_recording = Some(lock);
        let plan = WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source: WhisperRuntimeSource::Managed,
                backend: WhisperRuntimeBackend::Cpu,
                cli: binary,
                server: None,
                launch,
            },
            WhisperModelAsset {
                name: "small".to_string(),
                path: model,
                multilingual: true,
            },
            None,
        );
        let started = Instant::now();
        let error = WhisperEngine::with_plan(plan)
            .transcribe(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &options(LanguageChoice::Pinned(Language::ENGLISH)),
            )
            .unwrap_err();
        assert!(error.as_str().contains("canceled because recording"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn effective_launch_rehashes_the_runtime_immediately_before_inference() {
        let root = std::env::temp_dir().join(format!(
            "echo-whisper-effective-launch-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let cli = root.join("whisper-cli");
        let library = root.join("libwhisper.so");
        fs::write(&cli, b"cli").unwrap();
        fs::write(&library, b"library-v1").unwrap();
        let mut candidate = WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::System,
            backend: WhisperRuntimeBackend::Unknown,
            cli: cli.clone(),
            server: None,
            launch: whisper_runtime_launch(&cli),
        };
        candidate.launch.vulkan_driver_files = Some(root.join("driver.json"));
        candidate.launch.mesa_shader_cache_dir = Some(root.join("cache"));
        let original_identity = candidate.launch.identity_sha256.clone();
        let engine = WhisperEngine::with_plan(WhisperExecutionPlan::one_shot(
            candidate,
            WhisperModelAsset {
                name: "small".to_string(),
                path: root.join("model.bin"),
                multilingual: true,
            },
            None,
        ));
        fs::write(&library, b"library-v2").unwrap();
        let effective = engine.effective_runtime_launch(&cli).unwrap();
        assert_ne!(effective.identity_sha256, original_identity);
        assert_eq!(
            effective.vulkan_driver_files,
            Some(root.join("driver.json"))
        );
        assert_eq!(effective.mesa_shader_cache_dir, Some(root.join("cache")));
    }

    fn args_for_cache(dir: &Path) -> Vec<String> {
        let cache = ModelCache::at(dir);
        whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            cache.vad_model().as_deref(),
            &options(LanguageChoice::default()),
        )
    }

    fn vm_path(args: &[String]) -> Option<&str> {
        args.windows(2)
            .find(|pair| pair[0] == "-vm")
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn whisper_args_include_vad_when_silero_v6_present() {
        let dir = std::env::temp_dir().join(format!("echo-vad-v6-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let vad = dir.join("ggml-silero-v6.2.0.bin");
        fs::write(&vad, []).expect("dummy vad model");
        let args = args_for_cache(&dir);
        assert!(args.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&args).map(Path::new), Some(vad.as_path()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn whisper_args_include_vad_flags_only_when_model_is_some() {
        let vad = Path::new("ggml-silero-v6.2.0.bin");
        let with_vad = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            Some(vad),
            &options(LanguageChoice::default()),
        );
        assert!(with_vad.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&with_vad), Some("ggml-silero-v6.2.0.bin"));

        let without_vad = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(LanguageChoice::default()),
        );
        assert!(without_vad.iter().all(|arg| arg != "--vad" && arg != "-vm"));
    }

    #[test]
    fn whisper_args_omit_vad_when_cache_empty() {
        let dir = std::env::temp_dir().join(format!("echo-vad-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let args = args_for_cache(&dir);
        assert!(args.iter().all(|arg| arg != "--vad" && arg != "-vm"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn vad_model_prefers_v6_when_both_exist() {
        let dir = std::env::temp_dir().join(format!("echo-vad-prefer-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let v6 = dir.join("ggml-silero-v6.2.0.bin");
        let v5 = dir.join("ggml-silero-v5.1.2.bin");
        fs::write(&v6, []).expect("dummy v6");
        fs::write(&v5, []).expect("dummy v5");
        let args = args_for_cache(&dir);
        assert!(args.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&args).map(Path::new), Some(v6.as_path()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_file_finds_scanned_and_unconventional_names() {
        let dir = std::env::temp_dir().join(format!("echo-model-file-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-small.en-q5_1.bin"), []).unwrap();
        fs::write(dir.join("my-finetune.bin"), []).unwrap();
        let scanned = WhisperEngine::configured(ModelCache::at(&dir), "small.en-q5_1");
        assert_eq!(
            scanned.model_file().as_deref(),
            Some(dir.join("ggml-small.en-q5_1.bin").as_path())
        );
        let custom = WhisperEngine::configured(ModelCache::at(&dir), "my-finetune");
        assert_eq!(
            custom.model_file().as_deref(),
            Some(dir.join("my-finetune.bin").as_path())
        );
        let missing = WhisperEngine::configured(ModelCache::at(&dir), "large-v3");
        assert_eq!(missing.model_file(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    fn language_arg(args: &[String]) -> Option<&str> {
        args.windows(2)
            .find(|pair| pair[0] == "-l")
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn pinned_language_yields_dash_l_code() {
        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(german),
        );
        assert_eq!(language_arg(&args), Some("de"));
    }

    #[test]
    fn auto_language_yields_dash_l_auto() {
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(LanguageChoice::Auto),
        );
        assert_eq!(language_arg(&args), Some("auto"));
    }

    #[test]
    fn prompt_is_one_argument_and_survives_vad_retry_args() {
        let options = options_with_hint(LanguageChoice::Auto, "Claude Code");
        let first = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            Some(Path::new("vad.bin")),
            &options,
        );
        let retry = whisper_args(Path::new("model.bin"), Path::new("in.wav"), None, &options);
        for args in [&first, &retry] {
            assert_eq!(
                args.windows(2)
                    .find(|pair| pair[0] == "--prompt")
                    .map(|pair| pair[1].as_str()),
                Some("Claude Code")
            );
            assert_eq!(language_arg(args), Some("auto"));
        }
        assert!(first.iter().any(|arg| arg == "--vad"));
        assert!(retry.iter().all(|arg| arg != "--vad"));
    }

    #[test]
    fn empty_hints_omit_prompt() {
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(LanguageChoice::Auto),
        );
        assert!(args.iter().all(|arg| arg != "--prompt"));
    }

    #[test]
    fn normal_runs_preserve_runtime_tuning_defaults() {
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(LanguageChoice::Auto),
        );
        assert!(args
            .iter()
            .all(|arg| !matches!(arg.as_str(), "-t" | "-bs" | "-bo" | "-nf")));
        assert!(args.iter().all(|arg| arg != "--no-gpu"));
    }

    #[test]
    fn cpu_only_benchmark_plan_passes_the_upstream_flag() {
        let args = whisper_args_with_tuning(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            &options(LanguageChoice::Auto),
            WhisperTuning::runtime_defaults(),
            true,
        );
        assert!(args.iter().any(|arg| arg == "--no-gpu"));
    }

    #[test]
    fn args_never_contain_dl_or_a_translate_task() {
        // `-dl` bypasses the multilingual guard through an upstream bug, and
        // turbo models are not trained for translation and would silently
        // return the original language. Neither may ever be constructed.
        for choice in [
            LanguageChoice::Auto,
            LanguageChoice::Pinned(Language::ENGLISH),
            LanguageChoice::Pinned(Language::from_code("de").unwrap()),
        ] {
            let args = whisper_args(
                Path::new("m.bin"),
                Path::new("in.wav"),
                None,
                &options(choice),
            );
            assert!(!args.iter().any(|arg| arg == "-dl"), "{args:?}");
            assert!(!args.iter().any(|arg| arg == "--task"), "{args:?}");
        }
        let source = include_str!("whisper.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production.contains(concat!("--", "task")));
        assert!(!production.contains(concat!("\"-d", "l\"")));
    }

    #[test]
    fn english_only_model_refuses_non_english_before_spawning() {
        let dir = std::env::temp_dir().join(format!("echo-refuse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-base.en.bin"), []).unwrap();
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);

        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        let engine = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
        // The refusal must fire even with no whisper binary on PATH.
        match engine.transcribe(&pcm, &options(german)) {
            Err(EngineError::Infer(message)) => {
                assert!(message.contains("ggml-base.en.bin"), "msg={message}");
                assert!(message.contains("german"), "msg={message}");
            }
            other => panic!("expected refusal, got {other:?}"),
        }

        let auto = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
        match auto.transcribe(&pcm, &options(LanguageChoice::Auto)) {
            Err(EngineError::Infer(message)) => {
                assert!(
                    message.contains("automatic language detection"),
                    "msg={message}"
                )
            }
            other => panic!("expected refusal, got {other:?}"),
        }

        // Pinned English on an .en model is accepted by the preflight check.
        assert!(refuse_impossible_language(
            &dir.join("ggml-base.en.bin"),
            false,
            LanguageChoice::Pinned(Language::ENGLISH),
        )
        .is_ok());

        // A multilingual model takes any pinned language.
        assert!(refuse_impossible_language(&dir.join("ggml-small.bin"), true, german).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    fn fixture(name: &str) -> String {
        fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/whisper")
                .join(name),
        )
        .unwrap_or_else(|err| panic!("read {name}: {err}"))
    }

    #[test]
    fn parses_multilingual_result() {
        let parsed = parse_whisper_json(&fixture("multilingual.json")).unwrap();
        assert_eq!(parsed.text, "Claude Code.");
        assert_eq!(parsed.language.as_deref(), Some("de"));
        assert_eq!(parsed.model, "base");
        assert!(parsed.multilingual);
    }

    #[test]
    fn parses_english_only_result() {
        let parsed = parse_whisper_json(&fixture("english.json")).unwrap();
        assert_eq!(parsed.text, "Claude Code.");
        assert_eq!(parsed.language.as_deref(), Some("en"));
        assert_eq!(parsed.model, "base");
        assert!(!parsed.multilingual);
    }

    #[test]
    fn empty_transcription_hides_stale_language() {
        let parsed = parse_whisper_json(&fixture("empty_transcription.json")).unwrap();
        assert!(parsed.text.is_empty());
        assert_eq!(parsed.language, None);
        assert!(!parsed.multilingual);
    }

    #[test]
    fn detection_probability_comes_from_stderr() {
        let stderr = "whisper_full: auto-detected language: de (p = 0.958162)\nmore noise";
        assert_eq!(parse_detection_probability(stderr), Some(0.958_162));
        assert_eq!(parse_detection_probability("no detection here"), None);
        let parsed = finish_whisper(true, fixture("multilingual.json").as_bytes(), stderr)
            .expect("valid fixture");
        assert_eq!(parsed.language.as_deref(), Some("de"));
        assert_eq!(parsed.language_probability, Some(0.958_162));
    }

    #[test]
    fn malformed_json_is_a_named_error() {
        let err = parse_whisper_json("not json at all").unwrap_err();
        match err {
            EngineError::Infer(msg) => assert!(msg.starts_with("whisper json:"), "msg={msg}"),
            other => panic!("expected Infer, got {other:?}"),
        }
    }

    #[test]
    fn nonzero_exit_is_an_error_even_with_text() {
        let err = finish_whisper(false, fixture("english.json").as_bytes(), "decoder crashed")
            .unwrap_err();
        assert_eq!(err, EngineError::Infer("decoder crashed".into()));
    }

    #[test]
    fn only_vad_failures_allow_the_no_vad_retry() {
        assert!(should_retry_without_vad("failed to load VAD model"));
        assert!(should_retry_without_vad(
            "whisper_vad: failed to initialize VAD context"
        ));
        assert!(should_retry_without_vad("error: unknown argument: --vad"));
        assert!(!should_retry_without_vad("decoder crashed"));
        assert!(!should_retry_without_vad("model allocation failed"));
        assert!(!should_retry_without_vad(
            "decoder crashed after loading ggml-silero-v6.2.0.bin"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_timeout_kills_and_reaps_a_hung_process() {
        let started = Instant::now();
        let error = run_attempt(
            Path::new("/bin/sh"),
            None,
            vec!["-c".to_string(), "sleep 5".to_string()],
            false,
            std::time::Duration::from_millis(30),
        )
        .unwrap_err();
        assert!(error.as_str().contains("timed out"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}
