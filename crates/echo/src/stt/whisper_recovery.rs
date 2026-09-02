use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use echo_core::{
    DecodeOptions, Engine, EngineError, EngineId, Pcm16kMono, Transcript, WhisperRecoveryReason,
    WhisperRecoveryTelemetry, WhisperRuntimeBackend,
};

use super::whisper_plan::{QualifiedWhisperPlan, WhisperPlanDecision};
use super::whisper_quarantine::{AcceleratorKey, QuarantineReason, MAX_QUARANTINE_LIFETIME_SECS};
use super::{QuarantineStore, WhisperEngine};

pub struct RecoveringWhisperEngine {
    decision: WhisperPlanDecision,
    quarantine: QuarantineStore,
    now: fn() -> u64,
    process_quarantine: Arc<Mutex<BTreeMap<String, u64>>>,
}

static PROCESS_QUARANTINE: OnceLock<Arc<Mutex<BTreeMap<String, u64>>>> = OnceLock::new();

impl RecoveringWhisperEngine {
    #[must_use]
    pub fn new(decision: WhisperPlanDecision, quarantine: QuarantineStore) -> Self {
        Self {
            decision,
            quarantine,
            now: system_now,
            process_quarantine: Arc::clone(
                PROCESS_QUARANTINE.get_or_init(|| Arc::new(Mutex::new(BTreeMap::new()))),
            ),
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn with_clock(
        decision: WhisperPlanDecision,
        quarantine: QuarantineStore,
        now: fn() -> u64,
    ) -> Self {
        Self {
            decision,
            quarantine,
            now,
            process_quarantine: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    #[cfg(test)]
    fn quarantine_is_active(&self, now: u64) -> Result<bool, String> {
        match &self.decision {
            WhisperPlanDecision::ManagedCpu { .. } => Ok(false),
            WhisperPlanDecision::QualifiedAccelerator(plan) => {
                self.quarantine.is_active(&plan.identity_key, now)
            }
        }
    }
}

impl Engine for RecoveringWhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.decision.model_name().to_string(),
        }
    }

    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError> {
        self.transcribe_bounded(
            pcm,
            options,
            Instant::now() + std::time::Duration::from_secs(15 * 60),
            &|| false,
        )
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
        match &self.decision {
            WhisperPlanDecision::ManagedCpu { plan } => WhisperEngine::with_plan((**plan).clone())
                .transcribe_bounded(pcm, options, deadline, cancelled),
            WhisperPlanDecision::QualifiedAccelerator(plan) => {
                self.transcribe_qualified(plan, pcm, options, deadline, cancelled)
            }
        }
    }
}

impl RecoveringWhisperEngine {
    fn transcribe_qualified(
        &self,
        plan: &QualifiedWhisperPlan,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Transcript, EngineError> {
        let now = (self.now)();
        match self.process_quarantined(&plan.identity_key, now) {
            Ok(true) => {
                return Self::run_fallback(
                    plan,
                    pcm,
                    options,
                    false,
                    WhisperRecoveryReason::Quarantined,
                    deadline,
                    cancelled,
                );
            }
            Err(()) => {
                return Self::run_fallback(
                    plan,
                    pcm,
                    options,
                    false,
                    WhisperRecoveryReason::QuarantineUnreadable,
                    deadline,
                    cancelled,
                );
            }
            Ok(false) => {}
        }
        match self.quarantine.is_active(&plan.identity_key, now) {
            Ok(true) => {
                return Self::run_fallback(
                    plan,
                    pcm,
                    options,
                    false,
                    WhisperRecoveryReason::Quarantined,
                    deadline,
                    cancelled,
                );
            }
            Err(_) => {
                return Self::run_fallback(
                    plan,
                    pcm,
                    options,
                    false,
                    WhisperRecoveryReason::QuarantineUnreadable,
                    deadline,
                    cancelled,
                );
            }
            Ok(false) => {}
        }

        let accelerated = WhisperEngine::with_plan(plan.primary.clone())
            .transcribe_bounded(pcm, options, deadline, cancelled);
        let failure = match accelerated {
            Ok(mut transcript) => match validate_accelerated(plan, &transcript) {
                Ok(()) => {
                    attach_recovery(&mut transcript, &plan.identity_key, true, None);
                    return Ok(transcript);
                }
                Err(reason) => reason,
            },
            Err(error) => {
                // User cancellation and an exhausted caller deadline are not
                // accelerator failures. Do not quarantine the device or start
                // a fallback that inherits the same already-expired bound.
                if cancelled() || Instant::now() >= deadline {
                    return Err(error);
                }
                classify_error(&error)
            }
        };
        if let Ok(mut keys) = self.process_quarantine.lock() {
            keys.insert(
                plan.identity_key.as_str().to_string(),
                now.saturating_add(MAX_QUARANTINE_LIFETIME_SECS),
            );
        }
        let _ = self
            .quarantine
            .record_failure(&plan.identity_key, quarantine_reason(failure), now);
        Self::run_fallback(plan, pcm, options, true, failure, deadline, cancelled)
    }

    fn process_quarantined(&self, key: &AcceleratorKey, now: u64) -> Result<bool, ()> {
        self.process_quarantine
            .lock()
            .map(|mut keys| {
                keys.retain(|_, expires_at| *expires_at > now);
                keys.contains_key(key.as_str())
            })
            .map_err(|_| ())
    }

    fn run_fallback(
        plan: &QualifiedWhisperPlan,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        accelerated_attempted: bool,
        reason: WhisperRecoveryReason,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Transcript, EngineError> {
        let mut transcript = WhisperEngine::with_plan(plan.fallback.clone())
            .transcribe_bounded(pcm, options, deadline, cancelled)?;
        attach_recovery(
            &mut transcript,
            &plan.identity_key,
            accelerated_attempted,
            Some(reason),
        );
        Ok(transcript)
    }
}

fn validate_accelerated(
    plan: &QualifiedWhisperPlan,
    transcript: &Transcript,
) -> Result<(), WhisperRecoveryReason> {
    if transcript.engine
        != (EngineId::Whisper {
            model: plan.primary.model.name.clone(),
        })
    {
        return Err(WhisperRecoveryReason::IdentityMismatch);
    }
    let telemetry = transcript
        .detail
        .whisper
        .as_ref()
        .ok_or(WhisperRecoveryReason::IdentityMismatch)?;
    if telemetry.runtime.identity_sha256 != plan.primary.runtime.launch.identity_sha256 {
        return Err(WhisperRecoveryReason::IdentityMismatch);
    }
    if telemetry.runtime.backend != WhisperRuntimeBackend::Vulkan {
        return Err(WhisperRecoveryReason::CpuFallback);
    }
    let receipt = telemetry
        .runtime
        .vulkan_receipt
        .as_ref()
        .ok_or(WhisperRecoveryReason::MissingReceipt)?;
    if receipt != &plan.expected_receipt {
        return Err(WhisperRecoveryReason::ReceiptMismatch);
    }
    Ok(())
}

fn classify_error(error: &EngineError) -> WhisperRecoveryReason {
    let message = error.as_str();
    if message.contains("timed out") {
        WhisperRecoveryReason::Timeout
    } else if message.starts_with("whisper json:") {
        WhisperRecoveryReason::MalformedOutput
    } else {
        WhisperRecoveryReason::RuntimeFailure
    }
}

fn quarantine_reason(reason: WhisperRecoveryReason) -> QuarantineReason {
    match reason {
        WhisperRecoveryReason::Timeout => QuarantineReason::Timeout,
        WhisperRecoveryReason::MalformedOutput => QuarantineReason::MalformedOutput,
        WhisperRecoveryReason::MissingReceipt => QuarantineReason::MissingReceipt,
        WhisperRecoveryReason::ReceiptMismatch => QuarantineReason::ReceiptMismatch,
        WhisperRecoveryReason::CpuFallback => QuarantineReason::CpuFallback,
        WhisperRecoveryReason::IdentityMismatch => QuarantineReason::IdentityMismatch,
        WhisperRecoveryReason::RuntimeFailure
        | WhisperRecoveryReason::Quarantined
        | WhisperRecoveryReason::QuarantineUnreadable => QuarantineReason::RuntimeFailure,
    }
}

fn attach_recovery(
    transcript: &mut Transcript,
    identity_key: &AcceleratorKey,
    accelerated_attempted: bool,
    fallback_reason: Option<WhisperRecoveryReason>,
) {
    if let Some(telemetry) = &mut transcript.detail.whisper {
        telemetry.recovery = Some(WhisperRecoveryTelemetry {
            identity_key: identity_key.as_str().to_string(),
            accelerated_attempted,
            fallback_reason,
        });
    }
}

fn system_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use echo_core::{
        DecodeOptions, Engine, Language, LanguageChoice, Pcm16kMono, RecognitionHints,
        WhisperRecoveryReason, WhisperRuntimeBackend, WhisperRuntimeSource, WhisperVulkanReceipt,
    };

    use super::*;
    use crate::stt::{
        whisper_runtime_launch, AcceleratorKey, QuarantineStore, WhisperExecutionPlan,
        WhisperModelAsset, WhisperPlanDecision, WhisperRuntimeCandidate, WhisperTuning,
    };

    const NOW: u64 = 1_000;

    fn now() -> u64 {
        NOW
    }

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "echo-whisper-recovery-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn receipt(device_id: u32) -> WhisperVulkanReceipt {
        WhisperVulkanReceipt {
            schema_version: 1,
            backend: "vulkan".to_string(),
            selected_index: 0,
            vendor_id: 0x8086,
            device_id,
            api_version: 4_211_006,
            driver_version: 104_865_800,
            device_uuid: "8680a6460c0000000002000000000000".to_string(),
            driver_uuid: "ee99561e45e1e718c6121d36d8345582".to_string(),
            pipeline_cache_uuid: "35e9eb9761bf7afc9291ffc449ddf849".to_string(),
        }
    }

    fn receipt_line(value: &WhisperVulkanReceipt) -> String {
        format!(
            "echo_whisper_runtime_receipt: {}",
            serde_json::to_string(value).unwrap()
        )
    }

    fn json(text: &str) -> String {
        format!(
            "{{\"model\":{{\"type\":\"base\",\"multilingual\":false}},\"result\":{{\"language\":\"en\"}},\"transcription\":[{{\"text\":\" {text}\"}}]}}"
        )
    }

    fn script(
        root: &Path,
        name: &str,
        marker: &Path,
        stdout: &str,
        stderr: &str,
        sleep: bool,
        success: bool,
    ) -> PathBuf {
        let path = root.join(name);
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' run >> '{}'\n{}printf '%s' '{}'\nprintf '%s\\n' '{}' >&2\nexit {}\n",
            marker.display(),
            if sleep { "sleep 5\n" } else { "" },
            stdout,
            stderr,
            if success { 0 } else { 1 }
        );
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(target_os = "linux")]
    fn staged_script(
        root: &Path,
        name: &str,
        marker: &Path,
        after_marker: &str,
        stdout: &str,
        stderr: &str,
        success: bool,
    ) -> PathBuf {
        let path = root.join(name);
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' run >> '{}'\n{}printf '%s' '{}'\nprintf '%s\\n' '{}' >&2\nexit {}\n",
            marker.display(),
            after_marker,
            stdout,
            stderr,
            if success { 0 } else { 1 }
        );
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn vad_rejecting_script(
        root: &Path,
        marker: &Path,
        stdout: &str,
        success_stderr: &str,
    ) -> PathBuf {
        let path = root.join("gpu-vad-reject");
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' run >> '{}'\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"--vad\" ]; then\n    printf '%s\\n' 'failed to load VAD model' >&2\n    exit 1\n  fi\ndone\nprintf '%s' '{}'\nprintf '%s\\n' '{}' >&2\n",
            marker.display(), stdout, success_stderr
        );
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn plan(
        binary: PathBuf,
        source: WhisperRuntimeSource,
        backend: WhisperRuntimeBackend,
        model: &Path,
        force_cpu: bool,
    ) -> WhisperExecutionPlan {
        let mut plan = WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source,
                backend,
                launch: whisper_runtime_launch(&binary),
                cli: binary,
                server: None,
            },
            WhisperModelAsset {
                name: "base".to_string(),
                path: model.to_path_buf(),
                multilingual: true,
            },
            None,
        );
        plan.tuning = WhisperTuning {
            threads: std::num::NonZeroUsize::new(4),
            beam_size: Some(3),
            best_of: Some(5),
            no_fallback: Some(false),
        };
        plan.force_cpu = force_cpu;
        plan.timeout = Duration::from_millis(200);
        plan.allow_vad_retry = false;
        plan
    }

    fn key() -> AcceleratorKey {
        serde_json::from_str(&format!("\"{}\"", "a".repeat(64))).unwrap()
    }

    fn options() -> DecodeOptions {
        DecodeOptions {
            language: LanguageChoice::Pinned(Language::ENGLISH),
            hints: RecognitionHints::default(),
        }
    }

    #[cfg(target_os = "linux")]
    fn bounded_engine(
        root: &Path,
        gpu: PathBuf,
        cpu: PathBuf,
        expected: WhisperVulkanReceipt,
        timeout: Duration,
    ) -> (RecoveringWhisperEngine, AcceleratorKey, PathBuf) {
        let model = root.join("ggml-base.en.bin");
        fs::write(&model, []).unwrap();
        let mut primary = plan(
            gpu,
            WhisperRuntimeSource::System,
            WhisperRuntimeBackend::Vulkan,
            &model,
            false,
        );
        primary.timeout = timeout;
        let mut fallback = plan(
            cpu,
            WhisperRuntimeSource::Managed,
            WhisperRuntimeBackend::Cpu,
            &model,
            true,
        );
        fallback.timeout = timeout;
        let identity_key = key();
        let decision =
            WhisperPlanDecision::qualified(identity_key.clone(), primary, fallback, expected)
                .unwrap();
        let quarantine_path = root.join("quarantine.json");
        (
            RecoveringWhisperEngine::with_clock(
                decision,
                QuarantineStore::at(quarantine_path.clone()),
                now,
            ),
            identity_key,
            quarantine_path,
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn caller_cancellation_does_not_fallback_or_quarantine() {
        let root = scratch("bounded-cancel");
        let gpu_marker = root.join("gpu.marker");
        let cpu_marker = root.join("cpu.marker");
        let expected = receipt(0x46a6);
        let gpu_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&expected)
        );
        let gpu = script(
            &root,
            "gpu",
            &gpu_marker,
            &json("GPU"),
            &gpu_log,
            true,
            true,
        );
        let cpu = script(
            &root,
            "cpu",
            &cpu_marker,
            &json("CPU"),
            "whisper_model_load: CPU total size = 1 MB",
            false,
            true,
        );
        let (engine, identity_key, quarantine_path) =
            bounded_engine(&root, gpu, cpu, expected, Duration::from_secs(2));

        let started = Instant::now();
        let error = engine
            .transcribe_bounded(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &options(),
                Instant::now() + Duration::from_secs(2),
                &|| gpu_marker.exists(),
            )
            .unwrap_err();

        assert!(error.as_str().contains("canceled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(fs::read_to_string(&gpu_marker).unwrap(), "run\n");
        assert!(!cpu_marker.exists());
        assert!(!engine.process_quarantined(&identity_key, NOW).unwrap());
        assert!(!quarantine_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exhausted_global_deadline_does_not_fallback_or_quarantine() {
        let root = scratch("bounded-deadline");
        let gpu_marker = root.join("gpu.marker");
        let cpu_marker = root.join("cpu.marker");
        let expected = receipt(0x46a6);
        let gpu_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&expected)
        );
        let gpu = script(
            &root,
            "gpu",
            &gpu_marker,
            &json("GPU"),
            &gpu_log,
            true,
            true,
        );
        let cpu = script(
            &root,
            "cpu",
            &cpu_marker,
            &json("CPU"),
            "whisper_model_load: CPU total size = 1 MB",
            false,
            true,
        );
        let (engine, identity_key, quarantine_path) =
            bounded_engine(&root, gpu, cpu, expected, Duration::from_secs(2));

        let started = Instant::now();
        let error = engine
            .transcribe_bounded(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &options(),
                Instant::now() + Duration::from_millis(250),
                &|| false,
            )
            .unwrap_err();

        assert!(error.as_str().contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(fs::read_to_string(&gpu_marker).unwrap(), "run\n");
        assert!(!cpu_marker.exists());
        assert!(!engine.process_quarantined(&identity_key, NOW).unwrap());
        assert!(!quarantine_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fallback_inherits_the_original_global_deadline() {
        let root = scratch("bounded-fallback-deadline");
        let gpu_marker = root.join("gpu.marker");
        let cpu_marker = root.join("cpu.marker");
        let expected = receipt(0x46a6);
        let gpu = staged_script(
            &root,
            "gpu",
            &gpu_marker,
            "sleep 0.05\n",
            &json("GPU"),
            "decoder crashed",
            false,
        );
        // A fresh four-second attempt reaches the marker. The caller's
        // original two-second deadline cannot.
        let cpu_after_marker = format!(
            "sleep 2.5\nprintf '%s\\n' reset-budget >> '{}'\nsleep 5\n",
            cpu_marker.display()
        );
        let cpu = staged_script(
            &root,
            "cpu",
            &cpu_marker,
            &cpu_after_marker,
            &json("CPU"),
            "whisper_model_load: CPU total size = 1 MB",
            true,
        );
        let (engine, identity_key, quarantine_path) =
            bounded_engine(&root, gpu, cpu, expected, Duration::from_secs(4));

        let started = Instant::now();
        let error = engine
            .transcribe_bounded(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &options(),
                Instant::now() + Duration::from_secs(2),
                &|| false,
            )
            .unwrap_err();

        assert!(error.as_str().contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(fs::read_to_string(&gpu_marker).unwrap(), "run\n");
        assert_eq!(fs::read_to_string(&cpu_marker).unwrap(), "run\n");
        assert!(engine.process_quarantined(&identity_key, NOW).unwrap());
        assert!(engine.quarantine_is_active(NOW).unwrap());
        assert!(quarantine_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_receipt_returns_accelerated_transcript_without_cpu_retry() {
        let root = scratch("success");
        let model = root.join("ggml-base.en.bin");
        fs::write(&model, []).unwrap();
        let gpu_marker = root.join("gpu.marker");
        let cpu_marker = root.join("cpu.marker");
        let expected = receipt(0x46a6);
        let gpu_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&expected)
        );
        let gpu = script(
            &root,
            "gpu",
            &gpu_marker,
            &json("GPU"),
            &gpu_log,
            false,
            true,
        );
        let cpu = script(
            &root,
            "cpu",
            &cpu_marker,
            &json("CPU"),
            "whisper_model_load: CPU total size = 1 MB",
            false,
            true,
        );
        let decision = WhisperPlanDecision::qualified(
            key(),
            plan(
                gpu,
                WhisperRuntimeSource::System,
                WhisperRuntimeBackend::Vulkan,
                &model,
                false,
            ),
            plan(
                cpu,
                WhisperRuntimeSource::Managed,
                WhisperRuntimeBackend::Cpu,
                &model,
                true,
            ),
            expected,
        )
        .unwrap();
        let store = QuarantineStore::at(root.join("quarantine.json"));
        let engine = RecoveringWhisperEngine::with_clock(decision, store, now);
        let transcript = engine
            .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &options())
            .unwrap();
        assert_eq!(transcript.raw, "GPU");
        assert!(!cpu_marker.exists());
        let recovery = transcript.detail.whisper.unwrap().recovery.unwrap();
        assert!(recovery.accelerated_attempted);
        assert_eq!(recovery.fallback_reason, None);

        let auto = DecodeOptions {
            language: LanguageChoice::Auto,
            hints: RecognitionHints::default(),
        };
        let transcript = engine
            .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &auto)
            .unwrap();
        assert_eq!(transcript.raw, "GPU");
        assert_eq!(fs::read_to_string(&gpu_marker).unwrap().lines().count(), 2);
        assert!(!cpu_marker.exists());
        let recovery = transcript.detail.whisper.unwrap().recovery.unwrap();
        assert!(recovery.accelerated_attempted);
        assert_eq!(recovery.fallback_reason, None);
    }

    #[test]
    fn every_accelerator_failure_quarantines_once_and_runs_one_cpu_retry() {
        let expected = receipt(0x46a6);
        let valid_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&expected)
        );
        let wrong_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&receipt(0x9999))
        );
        let cases = [
            (
                "crash",
                json("GPU"),
                "decoder crashed".to_string(),
                false,
                false,
                WhisperRecoveryReason::RuntimeFailure,
            ),
            (
                "malformed",
                "not json".to_string(),
                valid_log.clone(),
                false,
                true,
                WhisperRecoveryReason::MalformedOutput,
            ),
            (
                "missing-receipt",
                json("GPU"),
                "whisper_backend_init_gpu: using Vulkan0 backend".to_string(),
                false,
                true,
                WhisperRecoveryReason::MissingReceipt,
            ),
            (
                "wrong-receipt",
                json("GPU"),
                wrong_log,
                false,
                true,
                WhisperRecoveryReason::ReceiptMismatch,
            ),
            (
                "cpu-fallback",
                json("GPU"),
                "whisper_model_load: CPU total size = 1 MB".to_string(),
                false,
                true,
                WhisperRecoveryReason::CpuFallback,
            ),
            (
                "timeout",
                json("GPU"),
                valid_log,
                true,
                true,
                WhisperRecoveryReason::Timeout,
            ),
        ];
        for (label, stdout, stderr, sleep, success, reason) in cases {
            let root = scratch(label);
            let model = root.join("ggml-base.en.bin");
            fs::write(&model, []).unwrap();
            let gpu_marker = root.join("gpu.marker");
            let cpu_marker = root.join("cpu.marker");
            let gpu = script(&root, "gpu", &gpu_marker, &stdout, &stderr, sleep, success);
            let cpu = script(
                &root,
                "cpu",
                &cpu_marker,
                &json("CPU"),
                "whisper_model_load: CPU total size = 1 MB",
                false,
                true,
            );
            let identity_key = key();
            let decision = WhisperPlanDecision::qualified(
                identity_key.clone(),
                plan(
                    gpu,
                    WhisperRuntimeSource::System,
                    WhisperRuntimeBackend::Vulkan,
                    &model,
                    false,
                ),
                plan(
                    cpu,
                    WhisperRuntimeSource::Managed,
                    WhisperRuntimeBackend::Cpu,
                    &model,
                    true,
                ),
                expected.clone(),
            )
            .unwrap();
            let store = QuarantineStore::at(root.join("quarantine.json"));
            let engine = RecoveringWhisperEngine::with_clock(decision, store, now);
            let transcript = engine
                .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &options())
                .unwrap();
            assert_eq!(transcript.raw, "CPU", "{label}");
            assert_eq!(
                fs::read_to_string(&cpu_marker).unwrap().lines().count(),
                1,
                "{label}"
            );
            let recovery = transcript.detail.whisper.unwrap().recovery.unwrap();
            assert!(recovery.accelerated_attempted, "{label}");
            assert_eq!(recovery.fallback_reason, Some(reason), "{label}");
            assert!(engine.quarantine_is_active(NOW).unwrap(), "{label}");
            if label == "crash" {
                fs::remove_file(root.join("quarantine.json")).unwrap();
                let transcript = engine
                    .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &options())
                    .unwrap();
                assert_eq!(transcript.raw, "CPU");
                assert_eq!(fs::read_to_string(&gpu_marker).unwrap().lines().count(), 1);
                assert_eq!(fs::read_to_string(&cpu_marker).unwrap().lines().count(), 2);
                let recovery = transcript.detail.whisper.unwrap().recovery.unwrap();
                assert!(!recovery.accelerated_attempted);
                assert_eq!(
                    recovery.fallback_reason,
                    Some(WhisperRecoveryReason::Quarantined)
                );
                assert!(!engine
                    .process_quarantined(&identity_key, NOW + MAX_QUARANTINE_LIFETIME_SECS,)
                    .unwrap());
            }
        }
    }

    #[test]
    fn qualified_vad_failure_does_not_retry_acceleration_without_vad() {
        let root = scratch("vad-contract");
        let model = root.join("ggml-base.bin");
        let vad = root.join("ggml-silero.bin");
        fs::write(&model, []).unwrap();
        fs::write(&vad, []).unwrap();
        let gpu_marker = root.join("gpu.marker");
        let cpu_marker = root.join("cpu.marker");
        let expected = receipt(0x46a6);
        let gpu_log = format!(
            "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
            receipt_line(&expected)
        );
        let gpu = vad_rejecting_script(&root, &gpu_marker, &json("GPU"), &gpu_log);
        let cpu = script(
            &root,
            "cpu",
            &cpu_marker,
            &json("CPU"),
            "whisper_model_load: CPU total size = 1 MB",
            false,
            true,
        );
        let mut primary = plan(
            gpu,
            WhisperRuntimeSource::System,
            WhisperRuntimeBackend::Vulkan,
            &model,
            false,
        );
        primary.vad = Some(vad.clone());
        let mut fallback = plan(
            cpu,
            WhisperRuntimeSource::Managed,
            WhisperRuntimeBackend::Cpu,
            &model,
            true,
        );
        fallback.vad = Some(vad);
        let decision = WhisperPlanDecision::qualified(key(), primary, fallback, expected).unwrap();
        let engine = RecoveringWhisperEngine::with_clock(
            decision,
            QuarantineStore::at(root.join("quarantine.json")),
            now,
        );
        let transcript = engine
            .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &options())
            .unwrap();
        assert_eq!(transcript.raw, "CPU");
        assert_eq!(fs::read_to_string(gpu_marker).unwrap().lines().count(), 1);
        assert_eq!(fs::read_to_string(cpu_marker).unwrap().lines().count(), 1);
    }

    #[test]
    fn existing_or_unreadable_quarantine_skips_acceleration() {
        for (label, corrupt, reason) in [
            (
                "already-quarantined",
                false,
                WhisperRecoveryReason::Quarantined,
            ),
            (
                "unreadable-quarantine",
                true,
                WhisperRecoveryReason::QuarantineUnreadable,
            ),
        ] {
            let root = scratch(label);
            let model = root.join("ggml-base.en.bin");
            fs::write(&model, []).unwrap();
            let gpu_marker = root.join("gpu.marker");
            let cpu_marker = root.join("cpu.marker");
            let expected = receipt(0x46a6);
            let gpu_log = format!(
                "ggml_vulkan: 0 = Intel Graphics | uma: 1\nwhisper_backend_init_gpu: using Vulkan0 backend\n{}",
                receipt_line(&expected)
            );
            let gpu = script(
                &root,
                "gpu",
                &gpu_marker,
                &json("GPU"),
                &gpu_log,
                false,
                true,
            );
            let cpu = script(
                &root,
                "cpu",
                &cpu_marker,
                &json("CPU"),
                "whisper_model_load: CPU total size = 1 MB",
                false,
                true,
            );
            let identity_key = key();
            let decision = WhisperPlanDecision::qualified(
                identity_key.clone(),
                plan(
                    gpu,
                    WhisperRuntimeSource::System,
                    WhisperRuntimeBackend::Vulkan,
                    &model,
                    false,
                ),
                plan(
                    cpu,
                    WhisperRuntimeSource::Managed,
                    WhisperRuntimeBackend::Cpu,
                    &model,
                    true,
                ),
                expected,
            )
            .unwrap();
            let path = root.join("quarantine.json");
            let store = QuarantineStore::at(path.clone());
            if corrupt {
                fs::write(&path, b"not json").unwrap();
            } else {
                store
                    .record_failure(&identity_key, QuarantineReason::RuntimeFailure, NOW)
                    .unwrap();
            }
            let engine = RecoveringWhisperEngine::with_clock(decision, store, now);
            let transcript = engine
                .transcribe(&Pcm16kMono::from_samples(vec![0; 160]), &options())
                .unwrap();
            assert_eq!(transcript.raw, "CPU", "{label}");
            assert!(!gpu_marker.exists(), "{label}");
            assert_eq!(fs::read_to_string(cpu_marker).unwrap().lines().count(), 1);
            let recovery = transcript.detail.whisper.unwrap().recovery.unwrap();
            assert!(!recovery.accelerated_attempted, "{label}");
            assert_eq!(recovery.fallback_reason, Some(reason), "{label}");
        }
    }
}
