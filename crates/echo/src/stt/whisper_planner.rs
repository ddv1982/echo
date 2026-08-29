use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use echo_core::{
    DecodeOptions, Engine, EngineError, EngineId, Pcm16kMono, Transcript,
    WhisperAccelerationPreference, WhisperCachedDecision, WhisperRuntimeBackend,
    WhisperRuntimeSource, WhisperSelectionTelemetry,
};

use super::backend::vulkan::{LocalVulkanRoute, VulkanBackend};
use super::whisper_accel_cache::{
    LocalSelectionKey, LocalSelectionStore, NewLocalRouteObservation, VulkanReceiptObservation,
};
use super::whisper_admission::AdmissionIdentityKey;
use super::whisper_identity::Sha256Digest;
use super::whisper_portable::{
    installed_package_root, qualified_contract_by_id, resolve_qualified_contract, sha256_file,
    InferenceContractRecord, InstalledPortableSelection,
};
use super::{
    QuarantineStore, RecoveringWhisperEngine, WhisperEngine, WhisperExecutionPlan,
    WhisperPlanDecision, WhisperRuntimeCandidate, WhisperTuning,
};

const FOREGROUND_GPU_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct WhisperAccelerationPlanner {
    package: InstalledPortableSelection,
    store: LocalSelectionStore,
}

pub(crate) struct ReceiptDrivenWhisperEngine {
    managed_cpu: WhisperExecutionPlan,
    planner: WhisperAccelerationPlanner,
    preference: WhisperAccelerationPreference,
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
        })
    }

    fn contract(
        &self,
        plan: &WhisperExecutionPlan,
    ) -> Result<Option<&InferenceContractRecord>, String> {
        if plan.tuning != WhisperTuning::runtime_defaults() {
            return Ok(None);
        }
        if let Some(view) = self.store.model_view(
            &plan.model.path,
            plan.vad.as_deref(),
            Some(&self.package.package.selection.execution_artifact.id),
        )? {
            return Ok(qualified_contract_by_id(
                &self.package.package.selection.inference_contracts,
                &view.inference_contract_id,
            ));
        }
        let model_sha256 = sha256_file(&plan.model.path)?;
        let vad_sha256 = plan.vad.as_deref().map(sha256_file).transpose()?;
        resolve_qualified_contract(
            &self.package.package.selection.inference_contracts,
            &model_sha256,
            &vad_sha256,
            None,
        )
    }

    fn backend(&self) -> VulkanBackend {
        VulkanBackend::bounded(
            self.package.probe.clone(),
            self.package.runtime_launch(),
            Duration::from_secs(15),
            Instant::now() + FOREGROUND_GPU_DEADLINE,
        )
    }

    fn restore_route(
        &self,
        observation: super::whisper_accel_cache::LocalRouteObservation,
    ) -> Result<LocalVulkanRoute, String> {
        let mut route = self.backend().restore(&observation)?;
        if test_fault("driver-change") {
            route.fingerprint.icd_library_sha256 =
                Sha256Digest::parse("f".repeat(64)).map_err(|error| error.to_string())?;
        }
        if test_fault("device-reorder") {
            route.selected_index = route.selected_index.saturating_add(9);
        }
        Ok(route)
    }

    fn cached_route(
        &self,
        contract: &InferenceContractRecord,
    ) -> Result<Option<LocalVulkanRoute>, String> {
        let execution = &self.package.package.selection.execution_artifact.id;
        let Some(observation) = self.store.latest_route(execution, &contract.id)? else {
            return Ok(None);
        };
        let runtime_key = AdmissionIdentityKey::parse(observation.key.as_str().to_string())?;
        if QuarantineStore::at(self.store.root().join("runtime-quarantine.json"))
            .is_active(&runtime_key, unix_time())?
        {
            return Ok(None);
        }
        if self
            .store
            .snapshot(&observation.key, unix_time())?
            .active_quarantine
            .is_some()
        {
            return Ok(None);
        }
        Ok(Some(self.restore_route(observation)?))
    }

    fn accelerated_engine(
        &self,
        managed_cpu: &WhisperExecutionPlan,
        contract: &InferenceContractRecord,
        route: LocalVulkanRoute,
        backend: &VulkanBackend,
    ) -> Result<(RecoveringWhisperEngine, LocalSelectionKey), String> {
        let mut ready = match route.cached_ready.clone() {
            Some(ready) => ready,
            None => backend.ready(&route)?,
        };
        let selected_index = ready.selected_index;
        if test_fault("wrong-receipt") {
            ready.device_id ^= 1;
        }
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
        let _ = self.store.append_route(NewLocalRouteObservation {
            execution_artifact_id: execution.clone(),
            inference_contract_id: contract.id.clone(),
            key: key.clone(),
            stable_receipt: route.receipt.clone(),
            ready_receipt: VulkanReceiptObservation {
                stable: route.receipt.clone(),
                selected_index,
            },
            fingerprint: route.fingerprint.clone(),
            manifest_path: route.manifest_path.clone(),
            library_path: route.library_path.clone(),
            observed_at: unix_time(),
        });
        let _ = self.store.write_model_view(
            &managed_cpu.model.path,
            managed_cpu.vad.as_deref(),
            execution.clone(),
            contract.id.clone(),
            key.clone(),
        );
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
        Ok((
            RecoveringWhisperEngine::new_local(
                decision,
                quarantine,
                self.store.clone(),
                key.clone(),
            ),
            key,
        ))
    }
}

impl ReceiptDrivenWhisperEngine {
    fn gpu(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        contract: &InferenceContractRecord,
        route: LocalVulkanRoute,
    ) -> Result<Transcript, EngineError> {
        let backend = self.planner.backend();
        let Ok((engine, key)) =
            self.planner
                .accelerated_engine(&self.managed_cpu, contract, route, &backend)
        else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Unknown, None),
            );
        };
        let mut transcript = engine.transcribe(pcm, options)?;
        let cached_decision = observed_cached_decision(&transcript);
        attach_selection(
            &mut transcript,
            selection(
                self.preference,
                cached_decision,
                Some(key.as_str().to_string()),
            ),
        );
        Ok(transcript)
    }

    fn cpu(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
        selection: WhisperSelectionTelemetry,
    ) -> Result<Transcript, EngineError> {
        let mut transcript =
            WhisperEngine::with_plan(self.managed_cpu.clone()).transcribe(pcm, options)?;
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
                selection(self.preference, WhisperCachedDecision::Cpu, None),
            );
        }
        let contract = self
            .planner
            .contract(&self.managed_cpu)
            .map_err(EngineError::Infer)?;
        let Some(contract) = contract else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Cpu, None),
            );
        };
        let cached = self.planner.cached_route(contract);
        let backend = self.planner.backend();
        let route = match cached {
            Ok(Some(route)) => Ok(route),
            Ok(None) | Err(_) => backend.enumerate().and_then(|routes| {
                routes
                    .into_iter()
                    .next()
                    .ok_or_else(|| "no Vulkan route is available".to_string())
            }),
        };
        let Ok(route) = route else {
            return self.cpu(
                pcm,
                options,
                selection(self.preference, WhisperCachedDecision::Unknown, None),
            );
        };
        self.gpu(pcm, options, contract, route)
    }
}

pub(crate) fn resolved_whisper_acceleration(
    override_preference: Option<WhisperAccelerationPreference>,
    file: Option<WhisperAccelerationPreference>,
) -> WhisperAccelerationPreference {
    override_preference
        .or(std::env::var("ECHO_WHISPER_ACCELERATION")
            .ok()
            .as_deref()
            .and_then(WhisperAccelerationPreference::parse))
        .or(file)
        .unwrap_or_else(super::whisper_acceleration_factory_default)
}

pub(crate) fn local_whisper_engine_from_process(
    managed_cpu: WhisperExecutionPlan,
    preference: WhisperAccelerationPreference,
) -> Option<Box<dyn Engine>> {
    if preference == WhisperAccelerationPreference::Cpu {
        let mut plan = managed_cpu;
        plan.force_cpu = true;
        return Some(Box::new(WhisperEngine::with_plan(plan)));
    }
    let echo_binary = std::env::current_exe().ok()?.canonicalize().ok()?;
    let package_root = installed_package_root(&echo_binary)?;
    let state_root = echo_core::data_dir().join("whisper-local-selection/v1");
    let planner = WhisperAccelerationPlanner::open(&package_root, state_root, &echo_binary).ok()?;
    Some(Box::new(ReceiptDrivenWhisperEngine {
        managed_cpu,
        planner,
        preference,
    }))
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
) -> WhisperSelectionTelemetry {
    WhisperSelectionTelemetry {
        preference,
        cached_decision,
        local_key,
        calibration_pending: false,
        proof_only: false,
    }
}

fn observed_cached_decision(transcript: &Transcript) -> WhisperCachedDecision {
    let Some(whisper) = transcript.detail.whisper.as_ref() else {
        return WhisperCachedDecision::Unknown;
    };
    if whisper
        .recovery
        .as_ref()
        .is_some_and(|recovery| recovery.fallback_reason.is_some())
    {
        return WhisperCachedDecision::Cpu;
    }
    match whisper.runtime.backend {
        WhisperRuntimeBackend::Vulkan => WhisperCachedDecision::Vulkan,
        WhisperRuntimeBackend::Cpu => WhisperCachedDecision::Cpu,
        _ => WhisperCachedDecision::Unknown,
    }
}

fn attach_selection(transcript: &mut Transcript, selection: WhisperSelectionTelemetry) {
    if let Some(whisper) = transcript.detail.whisper.as_mut() {
        whisper.selection = Some(selection);
    }
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn test_fault(expected: &str) -> bool {
    cfg!(debug_assertions) && std::env::var("ECHO_WHISPER_TEST_FAULT").as_deref() == Ok(expected)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use echo_core::{Language, RecognitionHints};

    use super::*;
    use crate::stt::whisper_accel_cache::{
        new_record_id, DriverIcdFingerprint, NewLocalRouteObservation, StableVulkanReceipt,
        VulkanReceiptObservation,
    };
    use crate::stt::whisper_identity::{
        CommitDigest, ExecutionArtifactId, ExecutionArtifactInput, InferenceContractId,
        InferenceContractInput, PackageType, SafeRelativePath, Sha256Digest, UuidDigest,
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
    fn cold_auto_does_not_spawn_calibration() {
        let root = scratch();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let cpu_marker = root.join("cpu-ran");
        let (planner, _) = planner_fixture(&root, &model);
        let engine = ReceiptDrivenWhisperEngine {
            managed_cpu: cpu_plan(&root, &model, &cpu_marker),
            planner: planner.clone(),
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
        assert!(planner.store.job_paths().unwrap().is_empty());
        let selection = transcript.detail.whisper.unwrap().selection.unwrap();
        assert_eq!(selection.preference, WhisperAccelerationPreference::Auto);
        assert_eq!(selection.cached_decision, WhisperCachedDecision::Unknown);
        assert!(!selection.calibration_pending);
        assert!(!selection.proof_only);
    }

    #[test]
    fn auto_language_does_not_force_cpu_by_policy() {
        let root = scratch();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let cpu_marker = root.join("cpu-ran");
        let (planner, _) = planner_fixture(&root, &model);
        let engine = ReceiptDrivenWhisperEngine {
            managed_cpu: cpu_plan(&root, &model, &cpu_marker),
            planner,
            preference: WhisperAccelerationPreference::Auto,
        };
        let transcript = engine
            .transcribe(
                &Pcm16kMono::from_samples(vec![0; 160]),
                &DecodeOptions {
                    language: echo_core::LanguageChoice::Auto,
                    hints: RecognitionHints::default(),
                },
            )
            .unwrap();
        let selection = transcript.detail.whisper.unwrap().selection.unwrap();
        assert_eq!(selection.cached_decision, WhisperCachedDecision::Unknown);
        assert!(!selection.calibration_pending);
    }

    #[test]
    fn recognition_hints_do_not_force_cpu_by_policy() {
        let root = scratch();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let cpu_marker = root.join("cpu-ran");
        let dictionary_path = root.join("dictionary.json");
        fs::write(
            &dictionary_path,
            r#"{"entries":[{"spoken":"clawed code","written":"Claude Code","created_at":1}]}"#,
        )
        .unwrap();
        let hints = RecognitionHints::from_dictionary(
            &echo_core::Dictionary::load_from(&dictionary_path).unwrap(),
        );
        assert!(!hints.is_empty());
        let (planner, _) = planner_fixture(&root, &model);
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
                    hints,
                },
            )
            .unwrap();
        let selection = transcript.detail.whisper.unwrap().selection.unwrap();
        assert_eq!(selection.cached_decision, WhisperCachedDecision::Unknown);
        assert!(!selection.calibration_pending);
    }

    #[test]
    fn cached_warm_planner_p95_is_bounded() {
        let root = scratch();
        let model = root.join("model.bin");
        fs::write(&model, b"model").unwrap();
        let marker = root.join("cpu-ran");
        let (planner, contract_id) = planner_fixture(&root, &model);
        let execution_id = planner
            .package
            .package
            .selection
            .execution_artifact
            .id
            .clone();
        let manifest = root.join("intel_icd.json");
        let library = root.join("libvulkan_intel.so");
        fs::write(&manifest, b"manifest").unwrap();
        fs::write(&library, b"library").unwrap();
        let stable_receipt = StableVulkanReceipt {
            backend: "vulkan".to_string(),
            vendor_id: 0x8086,
            device_id: 0x46a6,
            api_version: 1,
            driver_version: 2,
            device_uuid: UuidDigest::parse("1".repeat(32)).unwrap(),
            driver_uuid: UuidDigest::parse("2".repeat(32)).unwrap(),
            pipeline_cache_uuid: UuidDigest::parse("3".repeat(32)).unwrap(),
        };
        let fingerprint = DriverIcdFingerprint {
            drm_driver: "i915".to_string(),
            icd_manifest_sha256: Sha256Digest::parse("4".repeat(64)).unwrap(),
            icd_library_sha256: Sha256Digest::parse("5".repeat(64)).unwrap(),
        };
        let key =
            LocalSelectionKey::derive(&execution_id, &contract_id, &stable_receipt, &fingerprint)
                .unwrap();
        let ready_receipt = VulkanReceiptObservation {
            stable: stable_receipt.clone(),
            selected_index: 0,
        };
        planner
            .store
            .append_route(NewLocalRouteObservation {
                execution_artifact_id: execution_id.clone(),
                inference_contract_id: contract_id.clone(),
                key: key.clone(),
                stable_receipt,
                ready_receipt,
                fingerprint,
                manifest_path: manifest,
                library_path: library,
                observed_at: 1,
            })
            .unwrap();
        planner
            .store
            .write_model_view(&model, None, execution_id, contract_id.clone(), key)
            .unwrap();
        let plan = cpu_plan(&root, &model, &marker);
        let mut timings = (0..100)
            .map(|_| {
                let started = Instant::now();
                let contract = planner.contract(&plan).unwrap().unwrap();
                assert_eq!(contract.id, contract_id);
                assert!(planner.cached_route(contract).unwrap().is_some());
                started.elapsed()
            })
            .collect::<Vec<_>>();
        timings.sort();
        println!("cached_warm_planner_p95_us={}", timings[94].as_micros());
        assert!(timings[94] <= Duration::from_millis(25));
    }
}
