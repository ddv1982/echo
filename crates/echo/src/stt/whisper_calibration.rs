use std::fmt;
use std::fs;
use std::num::NonZeroUsize;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use echo_core::{
    DecodeOptions, Engine, EngineError, Language, LanguageChoice, RecognitionHints,
    WhisperRuntimeBackend, WhisperRuntimeSource,
};
use serde::{Deserialize, Serialize};

use super::backend::vulkan::VulkanBackend;
use super::whisper_accel_cache::{
    new_record_id, CalibrationLease, CalibrationVerdict, LocalSelectionKey, LocalSelectionStore,
    NewCalibrationObservation, NewLocalRouteObservation, VulkanReceiptObservation,
};
use super::whisper_admission::QuarantineReason;
use super::whisper_identity::{ExecutionArtifactId, InferenceContractId};
use super::whisper_portable::{
    installed_package_root, resolve_qualified_contract, InferenceContractRecord,
    InstalledPortableSelection,
};
use super::{
    WhisperEngine, WhisperExecutionPlan, WhisperModelAsset, WhisperRuntimeCandidate, WhisperTuning,
};

const JOB_SCHEMA_VERSION: u32 = 1;
const JOB_RESULT_SCHEMA_VERSION: u32 = 1;
const MAX_JOB_BYTES: u64 = 64 * 1024;
const OWNER_DEADLINE: Duration = Duration::from_secs(5 * 60);
const CALIBRATION_START_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CalibrationJob {
    schema_version: u32,
    pub job_id: String,
    pub package_root: PathBuf,
    pub state_root: PathBuf,
    pub echo_binary: PathBuf,
    pub execution_artifact_id: Option<ExecutionArtifactId>,
    pub inference_contract_id: Option<InferenceContractId>,
    pub model_name: String,
    pub model_path: PathBuf,
    pub model_multilingual: bool,
    pub vad_path: Option<PathBuf>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum JobResultStatus {
    Passed,
    Failed,
}

#[derive(Debug)]
enum CalibrationError {
    Interrupted,
    Gpu {
        reason: QuarantineReason,
        detail: String,
    },
    Other(String),
}

impl From<String> for CalibrationError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interrupted => formatter.write_str("calibration was interrupted"),
            Self::Gpu { detail, .. } | Self::Other(detail) => formatter.write_str(detail),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalibrationJobResult {
    schema_version: u32,
    job_id: String,
    status: JobResultStatus,
    completed_at: u64,
}

impl CalibrationJob {
    pub(crate) fn new(
        package_root: PathBuf,
        state_root: PathBuf,
        echo_binary: PathBuf,
        execution_artifact_id: ExecutionArtifactId,
        inference_contract_id: InferenceContractId,
        model: &WhisperModelAsset,
        vad_path: Option<PathBuf>,
    ) -> Self {
        Self {
            schema_version: JOB_SCHEMA_VERSION,
            job_id: new_record_id(),
            package_root,
            state_root,
            echo_binary,
            execution_artifact_id: Some(execution_artifact_id),
            inference_contract_id: Some(inference_contract_id),
            model_name: model.name.clone(),
            model_path: model.path.clone(),
            model_multilingual: model.multilingual,
            vad_path,
            created_at: unix_time(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn deferred(
        package_root: PathBuf,
        state_root: PathBuf,
        echo_binary: PathBuf,
        model: &WhisperModelAsset,
        vad_path: Option<PathBuf>,
    ) -> Self {
        Self {
            schema_version: JOB_SCHEMA_VERSION,
            job_id: new_record_id(),
            package_root,
            state_root,
            echo_binary,
            execution_artifact_id: None,
            inference_contract_id: None,
            model_name: model.name.clone(),
            model_path: model.path.clone(),
            model_multilingual: model.multilingual,
            vad_path,
            created_at: unix_time(),
        }
    }

    fn validate(&self, path: &Path) -> Result<(), String> {
        let expected_name = format!("{}.json", self.job_id);
        if self.schema_version != JOB_SCHEMA_VERSION
            || self.job_id.len() != 32
            || !self
                .job_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || path.file_name().and_then(|name| name.to_str()) != Some(&expected_name)
            || self.created_at == 0
            || self.model_name.is_empty()
            || !self.package_root.is_absolute()
            || !self.state_root.is_absolute()
            || !self.echo_binary.is_absolute()
            || !self.model_path.is_absolute()
            || self
                .vad_path
                .as_ref()
                .is_some_and(|path| !path.is_absolute())
        {
            return Err("invalid Whisper calibration job".to_string());
        }
        Ok(())
    }

    fn validate_authority(&self, path: &Path, executable: &Path) -> Result<(), String> {
        let expected_state = self.validate_publish_authority(executable)?;
        let expected_job = expected_state
            .join("jobs")
            .join(format!("{}.json", self.job_id))
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if path.canonicalize().map_err(|error| error.to_string())? != expected_job {
            return Err("calibration job path differs from its state record".to_string());
        }
        Ok(())
    }

    fn validate_publish_authority(&self, executable: &Path) -> Result<PathBuf, String> {
        let expected_state = echo_core::data_dir().join("whisper-local-selection/v1");
        fs::create_dir_all(expected_state.join("jobs")).map_err(|error| error.to_string())?;
        self.validate(
            &expected_state
                .join("jobs")
                .join(format!("{}.json", self.job_id)),
        )?;
        let expected_state = expected_state
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if self
            .state_root
            .canonicalize()
            .map_err(|error| error.to_string())?
            != expected_state
        {
            return Err("calibration job state root differs from Echo data".to_string());
        }
        let installed = installed_package_root(executable)
            .ok_or_else(|| "installed portable selection package is missing".to_string())?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if self
            .package_root
            .canonicalize()
            .map_err(|error| error.to_string())?
            != installed
        {
            return Err("calibration job package root is not installed beside Echo".to_string());
        }
        if self
            .echo_binary
            .canonicalize()
            .map_err(|error| error.to_string())?
            != executable
                .canonicalize()
                .map_err(|error| error.to_string())?
        {
            return Err("calibration job belongs to another Echo binary".to_string());
        }
        Ok(expected_state)
    }
}

pub(crate) fn publish_and_spawn(
    store: &LocalSelectionStore,
    job: &CalibrationJob,
) -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let authorized_state = job.validate_publish_authority(&executable)?;
    if store
        .root()
        .canonicalize()
        .map_err(|error| error.to_string())?
        != authorized_state
    {
        return Err("calibration store differs from Echo data".to_string());
    }
    let path = store.publish_job(&job.job_id, job)?;
    job.validate_authority(&path, &executable)?;
    let inherited = ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "LANG", "LC_ALL"]
        .into_iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name, value)))
        .collect::<Vec<_>>();
    let mut command = Command::new(executable);
    let mut child = command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .envs(inherited)
        .arg("whisper-calibrate")
        .arg("--job")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    std::thread::Builder::new()
        .name("echo-whisper-calibration-reaper".to_string())
        .spawn(move || {
            let _ = child.wait();
        })
        .map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn run_calibration_job(path: &Path) -> Result<(), String> {
    let owner_started = Instant::now();
    let requested = read_job(path)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    requested.validate_authority(path, &executable)?;
    std::thread::sleep(CALIBRATION_START_GRACE);
    if crate::rec::session_active() {
        return Ok(());
    }
    let store = LocalSelectionStore::at(requested.state_root.clone());
    let Some(package_lease) =
        wait_for_lease(owner_started, || store.try_claim_package_verification())?
    else {
        return Ok(());
    };
    let package = InstalledPortableSelection::open_cached(
        &requested.package_root,
        &executable,
        &requested.state_root,
    )?;
    let contract = contract_for(&requested, &package)?;
    drop(package_lease);
    let execution_artifact_id = &package.package.selection.execution_artifact.id;
    let Some(_lease) = wait_for_lease(owner_started, || {
        store.try_claim(execution_artifact_id, &contract.id)
    })?
    else {
        return Ok(());
    };
    let pending = pending_scope(&store, &requested, &package, &contract.id)?;
    let Some(job) = pending.first() else {
        return Ok(());
    };
    if crate::rec::session_active() {
        return Ok(());
    }
    let already_calibrated_key = store
        .model_view(
            &job.model_path,
            job.vad_path.as_deref(),
            Some(execution_artifact_id),
        )
        .ok()
        .flatten()
        .filter(|view| {
            view.execution_artifact_id == *execution_artifact_id
                && view.inference_contract_id == contract.id
        })
        .map(|view| view.key);
    let (status, calibrated_key) = if let Some(key) = already_calibrated_key {
        (JobResultStatus::Passed, Some(key))
    } else {
        match calibrate(job, &store, owner_started) {
            Ok(key) => (JobResultStatus::Passed, Some(key)),
            Err(CalibrationError::Interrupted) => return Ok(()),
            Err(error) => {
                eprintln!("whisper-calibrate: {error}");
                (JobResultStatus::Failed, None)
            }
        }
    };
    let resolved = pending_scope(&store, &requested, &package, &contract.id)?;
    if let Some(key) = calibrated_key {
        for pending_job in &resolved {
            store.write_model_view(
                &pending_job.model_path,
                pending_job.vad_path.as_deref(),
                execution_artifact_id.clone(),
                contract.id.clone(),
                key.clone(),
            )?;
        }
    }
    for pending_job in resolved {
        store.publish_job_result(
            &pending_job.job_id,
            &CalibrationJobResult {
                schema_version: JOB_RESULT_SCHEMA_VERSION,
                job_id: pending_job.job_id.clone(),
                status,
                completed_at: unix_time(),
            },
        )?;
    }
    Ok(())
}

fn wait_for_lease(
    started: Instant,
    mut acquire: impl FnMut() -> Result<Option<CalibrationLease>, String>,
) -> Result<Option<CalibrationLease>, String> {
    loop {
        if let Some(lease) = acquire()? {
            return Ok(Some(lease));
        }
        if should_stop(started) {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn pending_scope(
    store: &LocalSelectionStore,
    requested: &CalibrationJob,
    package: &InstalledPortableSelection,
    inference_contract_id: &InferenceContractId,
) -> Result<Vec<CalibrationJob>, String> {
    let execution_artifact_id = &package.package.selection.execution_artifact.id;
    let mut pending = Vec::new();
    for path in store.job_paths()? {
        let job = read_job(&path)?;
        if job.package_root != requested.package_root
            || job.echo_binary != requested.echo_binary
            || job
                .execution_artifact_id
                .as_ref()
                .is_some_and(|expected| expected != execution_artifact_id)
        {
            continue;
        }
        if contract_for(&job, package)?.id == *inference_contract_id {
            pending.push(job);
        }
    }
    pending.sort_by(|left, right| {
        (left.created_at, &left.job_id).cmp(&(right.created_at, &right.job_id))
    });
    Ok(pending)
}

fn calibrate(
    job: &CalibrationJob,
    store: &LocalSelectionStore,
    started: Instant,
) -> Result<LocalSelectionKey, CalibrationError> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if executable
        .canonicalize()
        .map_err(|error| error.to_string())?
        != job
            .echo_binary
            .canonicalize()
            .map_err(|error| error.to_string())?
    {
        return Err(CalibrationError::Other(
            "calibration job belongs to another Echo binary".to_string(),
        ));
    }
    let package =
        InstalledPortableSelection::open_cached(&job.package_root, &executable, &job.state_root)?;
    if job
        .execution_artifact_id
        .as_ref()
        .is_some_and(|expected| expected != &package.package.selection.execution_artifact.id)
    {
        return Err(CalibrationError::Other(
            "calibration execution artifact changed".to_string(),
        ));
    }
    let contract = contract_for(job, &package)?;
    if should_stop(started) {
        return Err(CalibrationError::Interrupted);
    }
    let mut launch = package.runtime_launch();
    launch.cancel_on_recording = Some(echo_core::data_dir().join("recording.lock"));
    let backend = VulkanBackend::bounded(
        package.probe.clone(),
        launch.clone(),
        Duration::from_secs(15),
        started + OWNER_DEADLINE,
    );
    let route = backend
        .enumerate()
        .map_err(|error| deadline_error(started, error))?
        .into_iter()
        .next()
        .ok_or_else(|| "calibration found no Vulkan route".to_string())?;
    let ready = backend
        .ready(&route)
        .map_err(|error| deadline_error(started, error))?;
    let key = LocalSelectionKey::derive(
        &package.package.selection.execution_artifact.id,
        &contract.id,
        &route.receipt,
        &route.fingerprint,
    )?;
    let audio =
        crate::audio::load_wav(&package.calibration_fixture).map_err(|error| error.to_string())?;
    let model = WhisperModelAsset {
        name: job.model_name.clone(),
        path: job.model_path.clone(),
        multilingual: job.model_multilingual,
    };
    let tuning = tuning(contract)?;
    let mut cpu = WhisperExecutionPlan::one_shot(
        WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Cpu,
            cli: package.runtime.clone(),
            server: None,
            launch: launch.clone(),
        },
        model.clone(),
        job.vad_path.clone(),
    );
    cpu.force_cpu = true;
    cpu.allow_vad_retry = false;
    cpu.tuning = tuning;
    cpu.timeout = remaining_owner_time(started)?;
    let options = DecodeOptions {
        language: LanguageChoice::Pinned(Language::ENGLISH),
        hints: RecognitionHints::default(),
    };
    if should_stop(started) {
        return Err(CalibrationError::Interrupted);
    }
    let cpu_result = WhisperEngine::with_plan(cpu)
        .transcribe(&audio.pcm, &options)
        .map_err(|error| calibration_engine_error(error, false))?;
    if should_stop(started) {
        return Err(CalibrationError::Interrupted);
    }
    let mut gpu_launch = launch;
    gpu_launch.vulkan_driver_files = Some(route.manifest_path.clone());
    gpu_launch.vulkan_selector = Some(route.selector.clone());
    gpu_launch.mesa_shader_cache_dir = Some(store.root().join("cache/mesa").join(key.as_str()));
    let mut gpu = WhisperExecutionPlan::one_shot(
        WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Vulkan,
            cli: package.runtime.clone(),
            server: None,
            launch: gpu_launch,
        },
        model,
        job.vad_path.clone(),
    );
    gpu.allow_vad_retry = false;
    gpu.tuning = tuning;
    gpu.timeout = remaining_owner_time(started)?;
    let ready_receipt = VulkanReceiptObservation {
        stable: route.receipt.clone(),
        selected_index: ready.selected_index,
    };
    let gpu_attempt: Result<_, CalibrationError> = (|| {
        let gpu_result = WhisperEngine::with_plan(gpu)
            .transcribe(&audio.pcm, &options)
            .map_err(|error| calibration_engine_error(error, true))?;
        let runtime = gpu_result
            .detail
            .whisper
            .as_ref()
            .map(|whisper| &whisper.runtime)
            .ok_or_else(|| CalibrationError::Gpu {
                reason: QuarantineReason::MalformedOutput,
                detail: "calibration GPU canary has no runtime telemetry".to_string(),
            })?;
        if runtime.backend != WhisperRuntimeBackend::Vulkan {
            return Err(CalibrationError::Gpu {
                reason: QuarantineReason::CpuFallback,
                detail: "calibration GPU canary internally fell back to CPU".to_string(),
            });
        }
        let receipt = runtime
            .vulkan_receipt
            .as_ref()
            .ok_or_else(|| CalibrationError::Gpu {
                reason: QuarantineReason::MissingReceipt,
                detail: "calibration GPU canary has no result receipt".to_string(),
            })?;
        let result_receipt = VulkanReceiptObservation {
            stable: super::backend::vulkan::stable_receipt(receipt)
                .map_err(CalibrationError::Other)?,
            selected_index: receipt.selected_index,
        };
        if result_receipt.stable != ready_receipt.stable {
            return Err(CalibrationError::Gpu {
                reason: QuarantineReason::ReceiptMismatch,
                detail: "calibration GPU result receipt differs from ready receipt".to_string(),
            });
        }
        Ok((gpu_result, result_receipt))
    })();
    let (gpu_result, result_receipt) = match gpu_attempt {
        Ok(result) => result,
        Err(CalibrationError::Interrupted) => return Err(CalibrationError::Interrupted),
        Err(error) => {
            let reason = match &error {
                CalibrationError::Gpu { reason, .. } => *reason,
                CalibrationError::Other(_) => QuarantineReason::RuntimeFailure,
                CalibrationError::Interrupted => unreachable!(),
            };
            store.append_calibration(NewCalibrationObservation {
                key: key.clone(),
                verdict: CalibrationVerdict::Failed,
                cpu_infer_ms: cpu_result.infer_ms.max(1),
                gpu_infer_ms: None,
                transcript_parity: None,
                ready_receipt: Some(ready_receipt),
                result_receipt: None,
                observed_at: unix_time(),
            })?;
            store.append_quarantine(key, reason, unix_time())?;
            return Err(error);
        }
    };
    let parity = cpu_result.raw == gpu_result.raw;
    let gpu_infer_ms = gpu_result.infer_ms.max(1);
    let verdict = if !parity {
        CalibrationVerdict::Failed
    } else if super::whisper_accel_cache::gpu_beats_cpu(cpu_result.infer_ms.max(1), gpu_infer_ms)
    {
        CalibrationVerdict::GpuEligible
    } else {
        CalibrationVerdict::CpuOnly
    };
    store.append_calibration(NewCalibrationObservation {
        key: key.clone(),
        verdict,
        cpu_infer_ms: cpu_result.infer_ms.max(1),
        gpu_infer_ms: Some(gpu_result.infer_ms.max(1)),
        transcript_parity: Some(parity),
        ready_receipt: Some(ready_receipt.clone()),
        result_receipt: Some(result_receipt),
        observed_at: unix_time(),
    })?;
    if parity {
        store.append_route(NewLocalRouteObservation {
            execution_artifact_id: package.package.selection.execution_artifact.id.clone(),
            inference_contract_id: contract.id.clone(),
            key: key.clone(),
            stable_receipt: route.receipt,
            ready_receipt,
            fingerprint: route.fingerprint,
            manifest_path: route.manifest_path,
            library_path: route.library_path,
            observed_at: unix_time(),
        })?;
        store.write_model_view(
            &job.model_path,
            job.vad_path.as_deref(),
            package.package.selection.execution_artifact.id.clone(),
            contract.id.clone(),
            key.clone(),
        )?;
        Ok(key)
    } else {
        store.append_quarantine(key, QuarantineReason::IdentityMismatch, unix_time())?;
        Err(CalibrationError::Gpu {
            reason: QuarantineReason::IdentityMismatch,
            detail: "calibration CPU and GPU transcripts differ".to_string(),
        })
    }
}

fn verify_inputs(job: &CalibrationJob, contract: &InferenceContractRecord) -> Result<(), String> {
    if super::whisper_portable::sha256_file(&job.model_path)?.as_str()
        != contract.value.model_sha256.as_str()
    {
        return Err("calibration model digest changed".to_string());
    }
    match (&job.vad_path, &contract.value.vad_sha256) {
        (Some(path), Some(expected))
            if super::whisper_portable::sha256_file(path)?.as_str() == expected.as_str() => {}
        (None, None) => {}
        _ => return Err("calibration VAD digest changed".to_string()),
    }
    Ok(())
}

fn contract_for<'a>(
    job: &CalibrationJob,
    package: &'a InstalledPortableSelection,
) -> Result<&'a InferenceContractRecord, String> {
    let model_sha256 = super::whisper_portable::sha256_file(&job.model_path)?;
    let vad_sha256 = job
        .vad_path
        .as_deref()
        .map(super::whisper_portable::sha256_file)
        .transpose()?;
    let Some(contract) = resolve_qualified_contract(
        &package.package.selection.inference_contracts,
        &model_sha256,
        &vad_sha256,
        job.inference_contract_id.as_ref(),
    )?
    else {
        return Err("calibration inference contract is missing or ambiguous".to_string());
    };
    verify_inputs(job, contract)?;
    Ok(contract)
}

fn tuning(contract: &InferenceContractRecord) -> Result<WhisperTuning, String> {
    Ok(WhisperTuning {
        threads: NonZeroUsize::new(usize::from(contract.value.tuning.threads)),
        beam_size: Some(
            u8::try_from(contract.value.tuning.beam_size)
                .map_err(|_| "calibration beam size is out of range".to_string())?,
        ),
        best_of: Some(
            u8::try_from(contract.value.tuning.best_of)
                .map_err(|_| "calibration best-of is out of range".to_string())?,
        ),
        no_fallback: Some(contract.value.tuning.no_fallback),
    })
}

fn read_job(path: &Path) -> Result<CalibrationJob, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_JOB_BYTES {
        return Err("invalid calibration job file".to_string());
    }
    let job: CalibrationJob =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    job.validate(path)?;
    Ok(job)
}

fn should_stop(started: Instant) -> bool {
    started.elapsed() >= OWNER_DEADLINE || crate::rec::session_active()
}

fn remaining_owner_time(started: Instant) -> Result<Duration, CalibrationError> {
    let remaining = OWNER_DEADLINE.saturating_sub(started.elapsed());
    if remaining.is_zero() || crate::rec::session_active() {
        return Err(CalibrationError::Interrupted);
    }
    Ok(remaining)
}

fn deadline_error(started: Instant, error: String) -> CalibrationError {
    if should_stop(started) || error.contains("deadline expired") {
        CalibrationError::Interrupted
    } else {
        CalibrationError::Other(error)
    }
}

fn calibration_engine_error(error: EngineError, gpu: bool) -> CalibrationError {
    let detail = error.to_string();
    if detail.contains("canceled because recording") {
        return CalibrationError::Interrupted;
    }
    if gpu {
        CalibrationError::Gpu {
            reason: if detail.contains("timed out") {
                QuarantineReason::Timeout
            } else {
                QuarantineReason::RuntimeFailure
            },
            detail,
        }
    } else {
        CalibrationError::Other(detail)
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn losing_owner_waits_for_scope_instead_of_stranding_its_job() {
        let root = std::env::temp_dir().join(format!(
            "echo-calibration-lease-{}-{}",
            std::process::id(),
            new_record_id()
        ));
        let store = LocalSelectionStore::at(root);
        let execution = ExecutionArtifactId::parse("1".repeat(64)).unwrap();
        let inference = InferenceContractId::parse("2".repeat(64)).unwrap();
        let first = store.try_claim(&execution, &inference).unwrap().unwrap();
        let waiting_store = store.clone();
        let waiting_execution = execution.clone();
        let waiting_inference = inference.clone();
        let waiter = std::thread::spawn(move || {
            wait_for_lease(Instant::now(), || {
                waiting_store.try_claim(&waiting_execution, &waiting_inference)
            })
            .unwrap()
            .is_some()
        });
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        assert!(waiter.join().unwrap());
    }
}
