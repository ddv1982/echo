use std::fs;
use std::num::NonZeroUsize;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use echo_core::{
    DecodeOptions, Engine, Language, LanguageChoice, RecognitionHints, WhisperRuntimeBackend,
    WhisperRuntimeSource,
};
use serde::{Deserialize, Serialize};

use super::backend::vulkan::VulkanBackend;
use super::whisper_accel_cache::{
    new_record_id, CalibrationVerdict, LocalSelectionKey, LocalSelectionStore,
    NewCalibrationObservation, NewLocalRouteObservation, VulkanReceiptObservation,
};
use super::whisper_admission::QuarantineReason;
use super::whisper_identity::{ExecutionArtifactId, InferenceContractId};
use super::whisper_portable::{InferenceContractRecord, InstalledPortableSelection};
use super::{
    WhisperEngine, WhisperExecutionPlan, WhisperModelAsset, WhisperRuntimeCandidate, WhisperTuning,
};

const JOB_SCHEMA_VERSION: u32 = 1;
const JOB_RESULT_SCHEMA_VERSION: u32 = 1;
const MAX_JOB_BYTES: u64 = 64 * 1024;
const OWNER_DEADLINE: Duration = Duration::from_secs(5 * 60);

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
}

pub(crate) fn publish_and_spawn(
    store: &LocalSelectionStore,
    job: &CalibrationJob,
) -> Result<PathBuf, String> {
    let path = store.publish_job(&job.job_id, job)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg("whisper-calibrate")
        .arg("--job")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn run_calibration_job(path: &Path) -> Result<(), String> {
    let owner_started = Instant::now();
    let requested = read_job(path)?;
    let store = LocalSelectionStore::at(requested.state_root.clone());
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let package = InstalledPortableSelection::open_cached(
        &requested.package_root,
        &executable,
        &requested.state_root,
    )?;
    let contract = contract_for(&requested, &package)?;
    let execution_artifact_id = &package.package.selection.execution_artifact.id;
    let Some(_lease) = store.try_claim(execution_artifact_id, &contract.id)? else {
        return Ok(());
    };
    let mut pending = store
        .job_paths()?
        .into_iter()
        .map(|path| read_job(&path).map(|job| (path, job)))
        .collect::<Result<Vec<_>, _>>()?;
    pending.retain(|(_, job)| {
        job.package_root == requested.package_root
            && job.model_path == requested.model_path
            && job.vad_path == requested.vad_path
            && job.echo_binary == requested.echo_binary
            && !store.job_is_complete(&job.job_id)
    });
    pending.sort_by(|(_, left), (_, right)| {
        (left.created_at, &left.job_id).cmp(&(right.created_at, &right.job_id))
    });
    let Some((_, job)) = pending.first() else {
        return Ok(());
    };
    if crate::rec::session_active() {
        return Ok(());
    }
    let status = match calibrate(job, &store, owner_started) {
        Ok(()) => JobResultStatus::Passed,
        Err(error)
            if error.contains("canceled because recording")
                || error.contains("calibration stopped") =>
        {
            return Ok(());
        }
        Err(error) => {
            eprintln!("whisper-calibrate: {error}");
            JobResultStatus::Failed
        }
    };
    for (_, pending_job) in pending {
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

fn calibrate(
    job: &CalibrationJob,
    store: &LocalSelectionStore,
    started: Instant,
) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if executable
        .canonicalize()
        .map_err(|error| error.to_string())?
        != job
            .echo_binary
            .canonicalize()
            .map_err(|error| error.to_string())?
    {
        return Err("calibration job belongs to another Echo binary".to_string());
    }
    let package =
        InstalledPortableSelection::open_cached(&job.package_root, &executable, &job.state_root)?;
    if job
        .execution_artifact_id
        .as_ref()
        .is_some_and(|expected| expected != &package.package.selection.execution_artifact.id)
    {
        return Err("calibration execution artifact changed".to_string());
    }
    let contract = contract_for(job, &package)?;
    if should_stop(started) {
        return Err("calibration stopped before device discovery".to_string());
    }
    let mut launch = package.runtime_launch();
    launch.cancel_on_recording = Some(echo_core::data_dir().join("recording.lock"));
    let backend = VulkanBackend::system(
        package.probe.clone(),
        launch.clone(),
        Duration::from_secs(15),
    );
    let route = backend
        .enumerate()?
        .into_iter()
        .next()
        .ok_or_else(|| "calibration found no Vulkan route".to_string())?;
    let ready = backend.ready(&route)?;
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
    let options = DecodeOptions {
        language: LanguageChoice::Pinned(Language::ENGLISH),
        hints: RecognitionHints::default(),
    };
    if should_stop(started) {
        return Err("calibration stopped before CPU canary".to_string());
    }
    let cpu_result = WhisperEngine::with_plan(cpu)
        .transcribe(&audio.pcm, &options)
        .map_err(|error| error.to_string())?;
    if should_stop(started) {
        return Err("calibration stopped before GPU canary".to_string());
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
    let ready_receipt = VulkanReceiptObservation {
        stable: route.receipt.clone(),
        selected_index: ready.selected_index,
    };
    let gpu_attempt = (|| {
        let gpu_result = WhisperEngine::with_plan(gpu)
            .transcribe(&audio.pcm, &options)
            .map_err(|error| error.to_string())?;
        let runtime = gpu_result
            .detail
            .whisper
            .as_ref()
            .map(|whisper| &whisper.runtime)
            .ok_or_else(|| "calibration GPU canary has no runtime telemetry".to_string())?;
        if runtime.backend != WhisperRuntimeBackend::Vulkan {
            return Err("calibration GPU canary internally fell back to CPU".to_string());
        }
        let receipt = runtime
            .vulkan_receipt
            .as_ref()
            .ok_or_else(|| "calibration GPU canary has no result receipt".to_string())?;
        let result_receipt = VulkanReceiptObservation {
            stable: super::backend::vulkan::stable_receipt(receipt)?,
            selected_index: receipt.selected_index,
        };
        if result_receipt.stable != ready_receipt.stable {
            return Err("calibration GPU result receipt differs from ready receipt".to_string());
        }
        Ok((gpu_result, result_receipt))
    })();
    let (gpu_result, result_receipt) = match gpu_attempt {
        Ok(result) => result,
        Err(error) => {
            if error.contains("canceled because recording") {
                return Err(error);
            }
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
            let reason = if error.contains("timed out") {
                QuarantineReason::Timeout
            } else if error.contains("fell back to CPU") {
                QuarantineReason::CpuFallback
            } else if error.contains("receipt") {
                QuarantineReason::ReceiptMismatch
            } else {
                QuarantineReason::RuntimeFailure
            };
            store.append_quarantine(key, reason, unix_time())?;
            return Err(error);
        }
    };
    let parity = cpu_result.raw == gpu_result.raw;
    let verdict = if parity {
        CalibrationVerdict::GpuEligible
    } else {
        CalibrationVerdict::Failed
    };
    store.append_calibration(NewCalibrationObservation {
        key: key.clone(),
        verdict,
        cpu_infer_ms: cpu_result.infer_ms.max(1),
        gpu_infer_ms: Some(gpu_result.infer_ms.max(1)),
        transcript_parity: Some(parity),
        ready_receipt: Some(ready_receipt),
        result_receipt: Some(result_receipt),
        observed_at: unix_time(),
    })?;
    if parity {
        store.append_route(NewLocalRouteObservation {
            execution_artifact_id: package.package.selection.execution_artifact.id.clone(),
            inference_contract_id: contract.id.clone(),
            key: key.clone(),
            stable_receipt: route.receipt,
            fingerprint: route.fingerprint,
            manifest_path: route.manifest_path,
            library_path: route.library_path,
            observed_at: unix_time(),
        })?;
        store.write_model_view(
            &job.model_path,
            package.package.selection.execution_artifact.id.clone(),
            contract.id.clone(),
            key.clone(),
        )?;
        Ok(())
    } else {
        store.append_quarantine(key, QuarantineReason::IdentityMismatch, unix_time())?;
        Err("calibration CPU and GPU transcripts differ".to_string())
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
    let matches = package
        .package
        .selection
        .inference_contracts
        .iter()
        .filter(|contract| {
            contract.value.model_sha256 == model_sha256
                && contract.value.vad_sha256 == vad_sha256
                && job
                    .inference_contract_id
                    .as_ref()
                    .is_none_or(|expected| expected == &contract.id)
        })
        .collect::<Vec<_>>();
    let [contract] = matches.as_slice() else {
        return Err("calibration inference contract is missing or ambiguous".to_string());
    };
    verify_inputs(job, contract)?;
    Ok(*contract)
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

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
