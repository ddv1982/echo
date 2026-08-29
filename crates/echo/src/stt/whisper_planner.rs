use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use echo_core::{
    DecodeOptions, Engine, EngineError, EngineId, Pcm16kMono, Transcript,
    WhisperAccelerationPreference, WhisperCachedDecision, WhisperRecoveryReason,
    WhisperRuntimeBackend, WhisperRuntimeSource, WhisperSelectionTelemetry,
};

use super::backend::vulkan::{LocalVulkanRoute, VulkanBackend};
use super::whisper_accel_cache::{LocalSelectionKey, LocalSelectionStore};
use super::whisper_admission::{AdmissionIdentityKey, QuarantineReason};
use super::whisper_calibration::{publish_and_spawn, CalibrationJob};
use super::whisper_portable::{sha256_file, InferenceContractRecord, InstalledPortableSelection};
use super::{
    QuarantineStore, RecoveringWhisperEngine, WhisperEngine, WhisperExecutionPlan,
    WhisperPlanDecision, WhisperRuntimeCandidate, WhisperTuning,
};

type CalibrationSpawner = dyn Fn(&LocalSelectionStore, &CalibrationJob) -> Result<PathBuf, String>;

#[derive(Clone)]
pub(crate) struct WhisperAccelerationPlanner {
    package: InstalledPortableSelection,
    store: LocalSelectionStore,
    echo_binary: PathBuf,
    spawner: Arc<CalibrationSpawner>,
}

pub(crate) struct ReceiptDrivenWhisperEngine {
    managed_cpu: WhisperExecutionPlan,
    planner: WhisperAccelerationPlanner,
    preference: WhisperAccelerationPreference,
}

struct DeferredAutoWhisperEngine {
    managed_cpu: WhisperExecutionPlan,
    package_root: PathBuf,
    store: LocalSelectionStore,
    echo_binary: PathBuf,
}

impl WhisperAccelerationPlanner {
    fn open(package_root: &Path, state_root: PathBuf, echo_binary: &Path) -> Result<Self, String> {
        Ok(Self {
            package: InstalledPortableSelection::open_cached(
                package_root,
                echo_binary,
                &state_root,
            )?,
            store: LocalSelectionStore::at(state_root),
            echo_binary: echo_binary.to_path_buf(),
            spawner: Arc::new(publish_and_spawn),
        })
    }

    fn contract(
        &self,
        plan: &WhisperExecutionPlan,
        options: &DecodeOptions,
    ) -> Result<Option<&InferenceContractRecord>, String> {
        if options.language == echo_core::LanguageChoice::Auto
            || !options.hints.is_empty()
            || plan.tuning != WhisperTuning::runtime_defaults()
        {
            return Ok(None);
        }
        if let Some(view) = self.store.model_view(&plan.model.path)? {
            return Ok(self
                .package
                .package
                .selection
                .inference_contracts
                .iter()
                .find(|contract| contract.id == view.inference_contract_id));
        }
        let model_sha256 = sha256_file(&plan.model.path)?;
        let vad_sha256 = plan.vad.as_deref().map(sha256_file).transpose()?;
        let matches = self
            .package
            .package
            .selection
            .inference_contracts
            .iter()
            .filter(|contract| {
                contract.value.model_sha256 == model_sha256
                    && contract.value.vad_sha256 == vad_sha256
                    && contract.value.protocol == "oneShotCli"
                    && contract.value.request_policy.language == "pinned"
                    && contract.value.request_policy.prompt == "empty"
                    && contract.value.request_policy.hints == "qualifiedOnly"
            })
            .collect::<Vec<_>>();
        let [contract] = matches.as_slice() else {
            return Ok(None);
        };
        Ok(Some(*contract))
    }

    fn backend(&self) -> VulkanBackend {
        VulkanBackend::system(
            self.package.probe.clone(),
            self.package.runtime_launch(),
            Duration::from_secs(15),
        )
    }

    fn cached_route(
        &self,
        contract: &InferenceContractRecord,
    ) -> Result<Option<LocalVulkanRoute>, String> {
        let execution = &self.package.package.selection.execution_artifact.id;
        let Some(observation) = self.store.latest_route(execution, &contract.id)? else {
            return Ok(None);
        };
        let snapshot = self.store.snapshot(&observation.key, unix_time())?;
        if snapshot.active_quarantine.is_some()
            || !snapshot
                .latest_calibration
                .as_ref()
                .is_some_and(|calibration| calibration.is_gpu_eligible())
        {
            return Ok(None);
        }
        self.backend().restore(&observation).map(Some)
    }

    fn schedule(&self, managed_cpu: &WhisperExecutionPlan, contract: &InferenceContractRecord) {
        let job = CalibrationJob::new(
            self.package.root.clone(),
            self.store.root().to_path_buf(),
            self.echo_binary.clone(),
            self.package.package.selection.execution_artifact.id.clone(),
            contract.id.clone(),
            &managed_cpu.model,
            managed_cpu.vad.clone(),
        );
        if let Err(error) = (self.spawner)(&self.store, &job) {
            eprintln!("whisper calibration spawn failed: {error}");
        }
    }

    fn accelerated_engine(
        &self,
        managed_cpu: &WhisperExecutionPlan,
        contract: &InferenceContractRecord,
        route: LocalVulkanRoute,
        require_ready_probe: bool,
    ) -> Result<(RecoveringWhisperEngine, LocalSelectionKey), String> {
        let backend = self.backend();
        let ready = if require_ready_probe {
            backend.ready(&route)?
        } else {
            echo_core::WhisperVulkanReceipt {
                schema_version: 1,
                backend: route.receipt.backend.clone(),
                selected_index: 0,
                vendor_id: route.receipt.vendor_id,
                device_id: route.receipt.device_id,
                api_version: route.receipt.api_version,
                driver_version: route.receipt.driver_version,
                device_uuid: route.receipt.device_uuid.as_str().to_string(),
                driver_uuid: route.receipt.driver_uuid.as_str().to_string(),
                pipeline_cache_uuid: route.receipt.pipeline_cache_uuid.as_str().to_string(),
            }
        };
        let execution = &self.package.package.selection.execution_artifact.id;
        let key =
            LocalSelectionKey::derive(execution, &contract.id, &route.receipt, &route.fingerprint)?;
        if self
            .store
            .snapshot(&key, unix_time())
            .map_or(true, |snapshot| snapshot.active_quarantine.is_some())
        {
            return Err("local Vulkan route is quarantined or unreadable".to_string());
        }
        let tuning = tuning(contract)?;
        let mut launch = self.package.runtime_launch();
        launch.vulkan_driver_files = Some(route.manifest_path);
        launch.vulkan_selector = Some(route.selector);
        launch.mesa_shader_cache_dir =
            Some(self.store.root().join("cache/mesa").join(key.as_str()));
        let mut primary = WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source: WhisperRuntimeSource::Managed,
                backend: WhisperRuntimeBackend::Vulkan,
                cli: self.package.runtime.clone(),
                server: None,
                launch,
            },
            managed_cpu.model.clone(),
            managed_cpu.vad.clone(),
        );
        primary.tuning = tuning;
        primary.timeout = managed_cpu.timeout;
        primary.allow_vad_retry = false;
        let mut fallback = managed_cpu.clone();
        fallback.tuning = tuning;
        fallback.force_cpu = true;
        fallback.allow_vad_retry = false;
        let identity = AdmissionIdentityKey::parse(key.as_str().to_string())?;
        let decision = WhisperPlanDecision::qualified(identity, primary, fallback, ready)?;
        let quarantine = QuarantineStore::at(self.store.root().join("runtime-quarantine.json"));
        Ok((RecoveringWhisperEngine::new(decision, quarantine), key))
    }
}

impl ReceiptDrivenWhisperEngine {
    fn cpu(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        selection: WhisperSelectionTelemetry,
        schedule: Option<&InferenceContractRecord>,
    ) -> Result<Transcript, EngineError> {
        let mut transcript =
            WhisperEngine::with_plan(self.managed_cpu.clone()).transcribe(pcm, options)?;
        if let Some(contract) = schedule {
            self.planner.schedule(&self.managed_cpu, contract);
        }
        attach_selection(&mut transcript, selection);
        Ok(transcript)
    }
}

impl Engine for ReceiptDrivenWhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.managed_cpu.model.name.clone(),
        }
    }

    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError> {
        if self.preference == WhisperAccelerationPreference::Cpu {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Cpu, None, false),
                None,
            );
        }
        let contract = self
            .planner
            .contract(&self.managed_cpu, options)
            .map_err(EngineError::Infer)?;
        let Some(contract) = contract else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Cpu, None, false),
                None,
            );
        };
        let cached = self.planner.cached_route(contract);
        if self.preference == WhisperAccelerationPreference::Auto {
            return match cached {
                Ok(Some(route)) => {
                    let key = LocalSelectionKey::derive(
                        &self.planner.package.package.selection.execution_artifact.id,
                        &contract.id,
                        &route.receipt,
                        &route.fingerprint,
                    )
                    .map_err(EngineError::Infer)?;
                    self.cpu(
                        pcm,
                        options,
                        selection(
                            self.preference,
                            WhisperCachedDecision::Vulkan,
                            Some(key.as_str().to_string()),
                            false,
                        ),
                        None,
                    )
                }
                Ok(None) | Err(_) => self.cpu(
                    pcm,
                    options,
                    selection(self.preference, WhisperCachedDecision::Unknown, None, true),
                    Some(contract),
                ),
            };
        }

        let route = match cached {
            Ok(Some(route)) => Ok((route, false)),
            Ok(None) | Err(_) => self.planner.backend().enumerate().and_then(|routes| {
                routes
                    .into_iter()
                    .next()
                    .map(|route| (route, true))
                    .ok_or_else(|| "no Vulkan route is available".to_string())
            }),
        };
        let Ok((route, require_ready_probe)) = route else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Unknown, None, false),
                None,
            );
        };
        let Ok((engine, key)) = self.planner.accelerated_engine(
            &self.managed_cpu,
            contract,
            route,
            require_ready_probe,
        ) else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Unknown, None, false),
                None,
            );
        };
        let mut transcript = engine.transcribe(pcm, options)?;
        if let Some(recovery) = transcript
            .detail
            .whisper
            .as_ref()
            .and_then(|whisper| whisper.recovery.as_ref())
        {
            if recovery.accelerated_attempted {
                if let Some(reason) = recovery.fallback_reason.and_then(quarantine_reason) {
                    let _ = self
                        .planner
                        .store
                        .append_quarantine(key.clone(), reason, unix_time());
                }
            }
        }
        attach_selection(
            &mut transcript,
            selection(
                self.preference,
                WhisperCachedDecision::Vulkan,
                Some(key.as_str().to_string()),
                false,
            ),
        );
        Ok(transcript)
    }
}

impl Engine for DeferredAutoWhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.managed_cpu.model.name.clone(),
        }
    }

    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError> {
        let cached = self
            .store
            .model_view(&self.managed_cpu.model.path)
            .ok()
            .flatten();
        let mut transcript =
            WhisperEngine::with_plan(self.managed_cpu.clone()).transcribe(pcm, options)?;
        let selection = if let Some(view) = cached {
            selection(
                WhisperAccelerationPreference::Auto,
                WhisperCachedDecision::Vulkan,
                Some(view.key.as_str().to_string()),
                false,
            )
        } else {
            let calibration_pending = options.language != echo_core::LanguageChoice::Auto
                && options.hints.is_empty()
                && self.managed_cpu.tuning == WhisperTuning::runtime_defaults();
            if calibration_pending {
                let job = CalibrationJob::deferred(
                    self.package_root.clone(),
                    self.store.root().to_path_buf(),
                    self.echo_binary.clone(),
                    &self.managed_cpu.model,
                    self.managed_cpu.vad.clone(),
                );
                if let Err(error) = publish_and_spawn(&self.store, &job) {
                    eprintln!("whisper calibration spawn failed: {error}");
                }
            }
            selection(
                WhisperAccelerationPreference::Auto,
                WhisperCachedDecision::Unknown,
                None,
                calibration_pending,
            )
        };
        attach_selection(&mut transcript, selection);
        Ok(transcript)
    }
}

pub(crate) fn local_whisper_engine_from_process(
    managed_cpu: WhisperExecutionPlan,
) -> Option<Box<dyn Engine>> {
    let preference = match std::env::var("ECHO_WHISPER_ACCELERATION").ok()?.as_str() {
        "auto" => WhisperAccelerationPreference::Auto,
        "gpu" => WhisperAccelerationPreference::Gpu,
        "cpu" => WhisperAccelerationPreference::Cpu,
        _ => return None,
    };
    if preference == WhisperAccelerationPreference::Cpu {
        return Some(Box::new(WhisperEngine::with_plan(managed_cpu)));
    }
    let echo_binary = std::env::current_exe().ok()?.canonicalize().ok()?;
    let package_root = package_root(&echo_binary)?;
    let state_root = echo_core::data_dir().join("whisper-local-selection/v1");
    if preference == WhisperAccelerationPreference::Auto {
        return Some(Box::new(DeferredAutoWhisperEngine {
            managed_cpu,
            package_root,
            store: LocalSelectionStore::at(state_root),
            echo_binary,
        }));
    }
    let planner = WhisperAccelerationPlanner::open(&package_root, state_root, &echo_binary).ok()?;
    Some(Box::new(ReceiptDrivenWhisperEngine {
        managed_cpu,
        planner,
        preference,
    }))
}

fn package_root(echo_binary: &Path) -> Option<PathBuf> {
    let parent = echo_binary.parent()?;
    let prefix = parent.parent()?;
    [
        parent.join("whisper-acceleration"),
        prefix.join("lib/echo/whisper-acceleration"),
        prefix.join("lib/io.github.ddv1982.echo/whisper-acceleration"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("portable-selection.v1.json").is_file())
}

fn tuning(contract: &InferenceContractRecord) -> Result<WhisperTuning, String> {
    Ok(WhisperTuning {
        threads: NonZeroUsize::new(usize::from(contract.value.tuning.threads)),
        beam_size: Some(
            u8::try_from(contract.value.tuning.beam_size)
                .map_err(|_| "Whisper beam size is out of range".to_string())?,
        ),
        best_of: Some(
            u8::try_from(contract.value.tuning.best_of)
                .map_err(|_| "Whisper best-of is out of range".to_string())?,
        ),
        no_fallback: Some(contract.value.tuning.no_fallback),
    })
}

fn selection(
    preference: WhisperAccelerationPreference,
    cached_decision: WhisperCachedDecision,
    local_key: Option<String>,
    calibration_pending: bool,
) -> WhisperSelectionTelemetry {
    WhisperSelectionTelemetry {
        preference,
        cached_decision,
        local_key,
        calibration_pending,
        proof_only: true,
    }
}

fn attach_selection(transcript: &mut Transcript, selection: WhisperSelectionTelemetry) {
    if let Some(whisper) = transcript.detail.whisper.as_mut() {
        whisper.selection = Some(selection);
    }
}

fn quarantine_reason(reason: WhisperRecoveryReason) -> Option<QuarantineReason> {
    match reason {
        WhisperRecoveryReason::RuntimeFailure => Some(QuarantineReason::RuntimeFailure),
        WhisperRecoveryReason::Timeout => Some(QuarantineReason::Timeout),
        WhisperRecoveryReason::MalformedOutput => Some(QuarantineReason::MalformedOutput),
        WhisperRecoveryReason::MissingReceipt => Some(QuarantineReason::MissingReceipt),
        WhisperRecoveryReason::ReceiptMismatch => Some(QuarantineReason::ReceiptMismatch),
        WhisperRecoveryReason::CpuFallback => Some(QuarantineReason::CpuFallback),
        WhisperRecoveryReason::IdentityMismatch => Some(QuarantineReason::IdentityMismatch),
        WhisperRecoveryReason::Quarantined
        | WhisperRecoveryReason::QuarantineUnreadable
        | WhisperRecoveryReason::PolicyMismatch => None,
    }
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use echo_core::{Language, RecognitionHints};

    use super::*;
    use crate::stt::whisper_accel_cache::new_record_id;
    use crate::stt::whisper_identity::{
        CommitDigest, ExecutionArtifactId, ExecutionArtifactInput, InferenceContractId,
        InferenceContractInput, PackageType, SafeRelativePath, Sha256Digest,
    };
    use crate::stt::whisper_portable::{
        CalibrationFixture, ExecutionArtifactRecord, InferenceContractRecord, LegacyExactIndex,
        PortableSelection, PortableSelectionBinding, PortableSelectionPackage,
    };
    use crate::stt::WhisperModelAsset;

    fn scratch() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "echo-whisper-planner-{}-{}",
            std::process::id(),
            new_record_id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixture() -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/whisper-v3-identities.json"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn planner_fixture(
        root: &Path,
        model: &Path,
        spawns: Arc<AtomicUsize>,
        cpu_marker: PathBuf,
    ) -> (WhisperAccelerationPlanner, InferenceContractId) {
        let fixture = fixture();
        let mut contract: InferenceContractInput =
            serde_json::from_value(fixture["cases"]["inferenceContract"]["input"].clone()).unwrap();
        contract.model_sha256 = sha256_file(model).unwrap();
        contract.vad_sha256 = None;
        let contract_id = InferenceContractId::of(&contract).unwrap();
        let execution: ExecutionArtifactInput =
            serde_json::from_value(fixture["cases"]["executionArtifact"]["input"].clone()).unwrap();
        let execution_id = ExecutionArtifactId::of(&execution).unwrap();
        let selection = PortableSelection {
            schema_version: 1,
            execution_artifact: ExecutionArtifactRecord {
                id: execution_id.clone(),
                value: execution,
            },
            inference_contracts: vec![InferenceContractRecord {
                id: contract_id.clone(),
                value: contract,
            }],
            calibration_fixture: CalibrationFixture {
                relative_path: SafeRelativePath::parse(
                    "calibration/english-canary.wav".to_string(),
                )
                .unwrap(),
                sha256: Sha256Digest::parse("8".repeat(64)).unwrap(),
            },
        };
        let package = PortableSelectionPackage {
            legacy_exact: LegacyExactIndex {
                schema_version: 1,
                execution_artifact_id: execution_id.clone(),
                records: Vec::new(),
            },
            binding: PortableSelectionBinding {
                schema_version: 1,
                package_type: PackageType::Deb,
                version: "test".to_string(),
                echo_commit: CommitDigest::parse("a".repeat(40)).unwrap(),
                echo_binary_sha256: Sha256Digest::parse("b".repeat(64)).unwrap(),
                portable_selection_sha256: Sha256Digest::parse("c".repeat(64)).unwrap(),
                legacy_exact_index_sha256: Sha256Digest::parse("d".repeat(64)).unwrap(),
                execution_artifact_id: execution_id,
                allowed_inference_contract_ids: vec![contract_id.clone()],
                source_acceleration_set_sha256: Sha256Digest::parse("e".repeat(64)).unwrap(),
                production_readiness: "local-selection-proof-only-until-pr16.4".to_string(),
            },
            selection,
        };
        let spawner = Arc::new(move |_: &LocalSelectionStore, _: &CalibrationJob| {
            assert!(cpu_marker.is_file());
            spawns.fetch_add(1, Ordering::SeqCst);
            Ok(PathBuf::from("job.json"))
        });
        (
            WhisperAccelerationPlanner {
                package: InstalledPortableSelection::for_test(
                    package,
                    root.to_path_buf(),
                    root.join("runtime/whisper-cli"),
                    root.join("runtime/probe"),
                    root.join("calibration/english-canary.wav"),
                ),
                store: LocalSelectionStore::at(root.join("state")),
                echo_binary: root.join("echo-desktop"),
                spawner,
            },
            contract_id,
        )
    }

    fn cpu_plan(root: &Path, model: &Path, marker: &Path) -> WhisperExecutionPlan {
        let binary = root.join("cpu");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf run > '{}'\nprintf '%s' '{{\"model\":{{\"type\":\"small\",\"multilingual\":true}},\"result\":{{\"language\":\"en\"}},\"transcription\":[{{\"text\":\" hello\"}}]}}'\nprintf '%s\\n' 'whisper_model_load: CPU total size = 1 MB' >&2\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let mut plan = WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source: WhisperRuntimeSource::Managed,
                backend: WhisperRuntimeBackend::Cpu,
                cli: binary.clone(),
                server: None,
                launch: crate::stt::whisper_runtime_launch(&binary),
            },
            WhisperModelAsset {
                name: "small".to_string(),
                path: model.to_path_buf(),
                multilingual: true,
            },
            None,
        );
        plan.force_cpu = true;
        plan
    }

    #[test]
    fn cold_auto_runs_cpu_before_scheduling_calibration() {
        let root = scratch();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let cpu_marker = root.join("cpu-ran");
        let spawns = Arc::new(AtomicUsize::new(0));
        let (planner, _) = planner_fixture(&root, &model, Arc::clone(&spawns), cpu_marker.clone());
        let engine = ReceiptDrivenWhisperEngine {
            managed_cpu: cpu_plan(&root, &model, &cpu_marker),
            planner,
            preference: WhisperAccelerationPreference::Auto,
        };
        let transcript = engine
            .transcribe(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &DecodeOptions {
                    language: echo_core::LanguageChoice::Pinned(Language::ENGLISH),
                    hints: RecognitionHints::default(),
                },
            )
            .unwrap();
        assert_eq!(transcript.raw, "hello");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        let selection = transcript.detail.whisper.unwrap().selection.unwrap();
        assert_eq!(selection.preference, WhisperAccelerationPreference::Auto);
        assert_eq!(selection.cached_decision, WhisperCachedDecision::Unknown);
        assert!(selection.calibration_pending);
        assert!(selection.proof_only);
    }
}
