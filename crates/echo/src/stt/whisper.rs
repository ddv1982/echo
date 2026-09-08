use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use std::{io::Read, thread::JoinHandle};

use echo_core::{
    strip_nonspeech, DecodeOptions, Engine, EngineError, EngineId, Language, LanguageChoice,
    Pcm16kMono, RunDetail, Transcript, WhisperAccelerationSkip, WhisperAttemptTelemetry,
    WhisperRetryReason, WhisperRunMode, WhisperRunTelemetry, WhisperRuntimeBackend,
    WhisperRuntimeSource, WhisperRuntimeTelemetry, WhisperTuningTelemetry,
};
use serde::Deserialize;

use super::cache::{parse_whisper_filename, ModelCache};
use super::whisper_behavior::{
    CLEARED_ENVIRONMENT_KEYS, CLEARED_ENVIRONMENT_PREFIXES, ONE_SHOT_TIMEOUT_SECS,
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

const STDERR_CAPTURE_LIMIT: usize = 64 * 1024;
const STDERR_OMISSION_MARKER: &[u8] = b"\n...[stderr omitted]...\n";

pub struct WhisperEngine {
    model: String,
    files: WhisperFiles,
    /// Set when this engine only exists because the GPU path declined the
    /// request, so the run reports why it is on the CPU.
    skipped_acceleration: Option<WhisperAccelerationSkip>,
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
            skipped_acceleration: None,
        }
    }

    #[must_use]
    pub fn with_plan(plan: WhisperExecutionPlan) -> Self {
        Self {
            model: plan.model.name.clone(),
            files: WhisperFiles::Explicit(Box::new(plan)),
            skipped_acceleration: None,
        }
    }

    /// Records that the user asked for the GPU and did not get it.
    #[must_use]
    pub fn skipped_acceleration(mut self, reason: WhisperAccelerationSkip) -> Self {
        self.skipped_acceleration = Some(reason);
        self
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
        self.selected_model_from_inventory(&cache.inventory())
    }

    pub(crate) fn selected_model_from_inventory(
        &self,
        inventory: &super::ModelInventory,
    ) -> Option<(PathBuf, bool)> {
        let WhisperFiles::Discover(cache) = &self.files else {
            return self.selected_model();
        };
        let model = self.model.as_str();
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
        self.transcribe_bounded(pcm, options, Instant::now() + self.timeout(), &|| false)
    }

    fn transcribe_bounded(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Transcript, EngineError> {
        if cancelled() {
            return Err(EngineError::Infer("Whisper runtime canceled".to_string()));
        }
        if Instant::now() >= deadline {
            return Err(EngineError::Infer(
                "Whisper runtime timed out before starting".to_string(),
            ));
        }
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
        let wav = write_temp_wav(pcm).map_err(EngineError::Infer)?;
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
            deadline,
            cancelled,
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
                deadline,
                cancelled,
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
                    skipped_acceleration: self.skipped_acceleration,
                }),
            },
        })
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
    execution_deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Output, WhisperAttemptTelemetry), EngineError> {
    let wall_started = Instant::now();
    let mut command = command_for_runtime(binary, launch);
    command.args(args);
    let deadline = (Instant::now() + timeout).min(execution_deadline);
    let recording_lock = launch.and_then(|launch| launch.cancel_on_recording.as_deref());
    let should_cancel = || cancelled() || recording_lock.is_some_and(crate::rec::session_active_at);
    let bounded = run_process_group_with_diagnostics(command, deadline, &should_cancel, true)
        .map_err(|error| match error {
            DiagnosticRunError::Cancelled(stderr)
                if recording_lock.is_some_and(crate::rec::session_active_at) =>
            {
                inference_error_with_stderr(
                    "Whisper calibration canceled because recording started".to_string(),
                    &stderr,
                )
            }
            DiagnosticRunError::Cancelled(stderr) => {
                inference_error_with_stderr("Whisper runtime canceled".to_string(), &stderr)
            }
            DiagnosticRunError::TimedOut(stderr) => inference_error_with_stderr(
                format!("Whisper runtime timed out after {} ms", timeout.as_millis()),
                &stderr,
            ),
            DiagnosticRunError::Io(message) => EngineError::Infer(message),
        })?;
    let output = bounded.output;
    let telemetry = WhisperAttemptTelemetry {
        vad,
        process_start_ms: bounded.process_start_ms,
        child_wall_ms: elapsed_ms(wall_started),
        success: output.status.success(),
        exit_code: output.status.code(),
        retry_reason: None,
    };
    Ok((output, telemetry))
}

pub(super) struct BoundedProcessOutput {
    pub(super) output: Output,
    pub(super) process_start_ms: u64,
}

#[derive(Debug)]
pub(super) enum ProcessRunError {
    Cancelled,
    TimedOut,
    Io(String),
}

#[derive(Debug)]
enum DiagnosticRunError {
    Cancelled(Vec<u8>),
    TimedOut(Vec<u8>),
    Io(String),
}

/// Spawn one isolated process group, drain both output pipes, and keep direct
/// ownership of `Child` so every post-spawn error can explicitly wait it.
pub(super) fn run_process_group(
    command: Command,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<BoundedProcessOutput, ProcessRunError> {
    run_process_group_with_diagnostics(command, deadline, cancelled, false).map_err(|error| {
        match error {
            DiagnosticRunError::Cancelled(_) => ProcessRunError::Cancelled,
            DiagnosticRunError::TimedOut(_) => ProcessRunError::TimedOut,
            DiagnosticRunError::Io(message) => ProcessRunError::Io(message),
        }
    })
}

fn run_process_group_with_diagnostics(
    mut command: Command,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    bound_stderr: bool,
) -> Result<BoundedProcessOutput, DiagnosticRunError> {
    if cancelled() {
        return Err(DiagnosticRunError::Cancelled(Vec::new()));
    }
    if Instant::now() >= deadline {
        return Err(DiagnosticRunError::TimedOut(Vec::new()));
    }
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let spawn_started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| DiagnosticRunError::Io(error.to_string()))?;
    let process_start_ms = elapsed_ms(spawn_started);
    let raw_pid = match i32::try_from(child.id()) {
        Ok(raw_pid) => raw_pid,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DiagnosticRunError::Io(
                "child process ID is out of range".to_string(),
            ));
        }
    };
    let Some(pid) = rustix::process::Pid::from_raw(raw_pid) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(DiagnosticRunError::Io(
            "child process ID is zero".to_string(),
        ));
    };
    let Some(stdout) = child.stdout.take() else {
        kill_group_and_reap(pid, &mut child, false);
        return Err(DiagnosticRunError::Io(
            "bounded child stdout pipe is missing".to_string(),
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        kill_group_and_reap(pid, &mut child, false);
        return Err(DiagnosticRunError::Io(
            "bounded child stderr pipe is missing".to_string(),
        ));
    };
    let stdout = match spawn_pipe_reader(stdout) {
        Ok(reader) => reader,
        Err(error) => {
            kill_group_and_reap(pid, &mut child, false);
            return Err(DiagnosticRunError::Io(error.to_string()));
        }
    };
    let stderr = match if bound_stderr {
        spawn_bounded_stderr_reader(stderr)
    } else {
        spawn_pipe_reader(stderr)
    } {
        Ok(reader) => reader,
        Err(error) => {
            kill_group_and_reap(pid, &mut child, false);
            let _ = stdout.join();
            return Err(DiagnosticRunError::Io(error.to_string()));
        }
    };
    let mut status = None;
    loop {
        let stop = if cancelled() {
            Some(false)
        } else if Instant::now() >= deadline {
            Some(true)
        } else {
            None
        };
        if let Some(stop) = stop {
            kill_group_and_reap(pid, &mut child, status.is_some());
            // Joining after SIGKILL guarantees no descendant still holds an
            // inherited output pipe when this call returns.
            let _ = stdout.join();
            let stderr = join_pipe_reader(stderr).unwrap_or_default();
            return Err(if stop {
                DiagnosticRunError::TimedOut(stderr)
            } else {
                DiagnosticRunError::Cancelled(stderr)
            });
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(exit) => status = exit,
                Err(error) => {
                    kill_group_and_reap(pid, &mut child, false);
                    let _ = stdout.join();
                    let _ = stderr.join();
                    return Err(DiagnosticRunError::Io(error.to_string()));
                }
            }
        }
        if let Some(status) = status.filter(|_| stdout.is_finished() && stderr.is_finished()) {
            let stdout = join_pipe_reader(stdout)?;
            let stderr = join_pipe_reader(stderr)?;
            return Ok(BoundedProcessOutput {
                output: Output {
                    status,
                    stdout,
                    stderr,
                },
                process_start_ms,
            });
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25)),
        );
    }
}

fn spawn_pipe_reader(
    mut pipe: impl Read + Send + 'static,
) -> std::io::Result<JoinHandle<std::io::Result<Vec<u8>>>> {
    std::thread::Builder::new()
        .name("echo-stt-output".to_string())
        .spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
}

fn join_pipe_reader(
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, DiagnosticRunError> {
    reader
        .join()
        .map_err(|_| DiagnosticRunError::Io("child output thread panicked".to_string()))?
        .map_err(|error| DiagnosticRunError::Io(error.to_string()))
}

struct BoundedStderr {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    omitted: usize,
}

impl BoundedStderr {
    fn new() -> Self {
        Self {
            head: Vec::with_capacity(Self::head_limit()),
            tail: VecDeque::with_capacity(Self::tail_limit()),
            omitted: 0,
        }
    }

    fn head_limit() -> usize {
        (STDERR_CAPTURE_LIMIT - STDERR_OMISSION_MARKER.len()) / 2
    }

    fn tail_limit() -> usize {
        STDERR_CAPTURE_LIMIT - STDERR_OMISSION_MARKER.len() - Self::head_limit()
    }

    fn extend(&mut self, mut bytes: &[u8]) {
        let head_remaining = Self::head_limit().saturating_sub(self.head.len());
        let take_head = head_remaining.min(bytes.len());
        self.head.extend_from_slice(&bytes[..take_head]);
        bytes = &bytes[take_head..];
        for byte in bytes {
            if self.tail.len() == Self::tail_limit() {
                self.tail.pop_front();
                self.omitted = self.omitted.saturating_add(1);
            }
            self.tail.push_back(*byte);
        }
    }

    fn finish(self) -> Vec<u8> {
        let mut result = self.head;
        if self.omitted > 0 {
            result.extend_from_slice(STDERR_OMISSION_MARKER);
        }
        result.extend(self.tail);
        result
    }
}

fn spawn_bounded_stderr_reader(
    mut pipe: impl Read + Send + 'static,
) -> std::io::Result<JoinHandle<std::io::Result<Vec<u8>>>> {
    std::thread::Builder::new()
        .name("echo-stt-stderr".to_string())
        .spawn(move || {
            let mut capture = BoundedStderr::new();
            let mut buffer = [0_u8; 8 * 1024];
            loop {
                let read = pipe.read(&mut buffer)?;
                if read == 0 {
                    return Ok(capture.finish());
                }
                capture.extend(&buffer[..read]);
            }
        })
}

fn kill_group_and_reap(pid: rustix::process::Pid, child: &mut Child, already_reaped: bool) {
    if rustix::process::kill_process_group(pid, rustix::process::Signal::KILL).is_err()
        && !already_reaped
    {
        let _ = child.kill();
    }
    if !already_reaped {
        let _ = child.wait();
    }
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
        Instant::now() + timeout,
        &|| false,
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
        Instant::now() + timeout,
        &|| false,
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
    let mut parsed = parse_whisper_stdout(stdout).map_err(|error| match error {
        EngineError::Infer(message) => inference_error_with_stderr(message, stderr.as_bytes()),
        EngineError::Missing => EngineError::Missing,
    })?;
    parsed.language_probability = parse_detection_probability(stderr);
    Ok(parsed)
}

fn inference_error_with_stderr(message: String, stderr: &[u8]) -> EngineError {
    if stderr.is_empty() {
        return EngineError::Infer(message);
    }
    EngineError::Infer(format!(
        "{message}\nWhisper stderr:\n{}",
        String::from_utf8_lossy(stderr).trim_end()
    ))
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
#[path = "whisper_tests.rs"]
mod tests;
