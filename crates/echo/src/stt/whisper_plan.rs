use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use echo_core::{WhisperRuntimeBackend, WhisperRuntimeSource, WhisperVulkanReceipt};

use super::whisper_admission::AdmissionIdentityKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperExecutionPlan {
    pub runtime: WhisperRuntimeCandidate,
    pub model: WhisperModelAsset,
    pub vad: Option<PathBuf>,
    pub tuning: WhisperTuning,
    pub protocol: WhisperProtocol,
    pub force_cpu: bool,
    pub timeout: Duration,
    pub allow_vad_retry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhisperPlanDecision {
    ManagedCpu {
        plan: Box<WhisperExecutionPlan>,
    },
    QualifiedAccelerator(Box<QualifiedWhisperPlan>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedWhisperPlan {
    pub(crate) identity_key: AdmissionIdentityKey,
    pub(crate) primary: WhisperExecutionPlan,
    pub(crate) fallback: WhisperExecutionPlan,
    pub(crate) expected_receipt: WhisperVulkanReceipt,
}

impl WhisperPlanDecision {
    pub fn managed_cpu(plan: WhisperExecutionPlan) -> Result<Self, String> {
        if plan.runtime.source != WhisperRuntimeSource::Managed
            || plan.runtime.backend != WhisperRuntimeBackend::Cpu
            || plan.protocol != WhisperProtocol::OneShotCli
        {
            return Err("managed CPU decision requires the managed one-shot CPU runtime".to_string());
        }
        Ok(Self::ManagedCpu {
            plan: Box::new(plan),
        })
    }

    pub fn qualified(
        identity_key: AdmissionIdentityKey,
        primary: WhisperExecutionPlan,
        fallback: WhisperExecutionPlan,
        expected_receipt: WhisperVulkanReceipt,
    ) -> Result<Self, String> {
        if primary.runtime.backend != WhisperRuntimeBackend::Vulkan
            || primary.protocol != WhisperProtocol::OneShotCli
            || primary.force_cpu
            || primary.runtime.launch.identity_sha256.is_none()
            || primary.allow_vad_retry
        {
            return Err("qualified primary must be an identified one-shot Vulkan runtime".to_string());
        }
        if fallback.runtime.source != WhisperRuntimeSource::Managed
            || fallback.runtime.backend != WhisperRuntimeBackend::Cpu
            || fallback.protocol != WhisperProtocol::OneShotCli
            || !fallback.force_cpu
            || fallback.runtime.launch.identity_sha256.is_none()
            || fallback.allow_vad_retry
        {
            return Err("qualified fallback must be the identified managed CPU runtime".to_string());
        }
        if primary.model != fallback.model
            || primary.vad != fallback.vad
            || primary.tuning != fallback.tuning
            || primary.protocol != fallback.protocol
            || primary.timeout != fallback.timeout
        {
            return Err("accelerated and CPU plans must share one decoding contract".to_string());
        }
        if expected_receipt.schema_version != 1
            || expected_receipt.backend != "vulkan"
            || expected_receipt.vendor_id == 0
            || expected_receipt.device_id == 0
        {
            return Err("qualified plan has an invalid Vulkan receipt".to_string());
        }
        Ok(Self::QualifiedAccelerator(Box::new(QualifiedWhisperPlan {
            identity_key,
            primary,
            fallback,
            expected_receipt,
        })))
    }

    #[must_use]
    pub fn model_name(&self) -> &str {
        match self {
            Self::ManagedCpu { plan } => &plan.model.name,
            Self::QualifiedAccelerator(plan) => &plan.primary.model.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperModelAsset {
    pub name: String,
    pub path: PathBuf,
    pub multilingual: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperRuntimeCandidate {
    pub source: WhisperRuntimeSource,
    pub backend: WhisperRuntimeBackend,
    pub cli: PathBuf,
    pub server: Option<PathBuf>,
    pub launch: WhisperRuntimeLaunch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WhisperRuntimeLaunch {
    pub library_dir: Option<PathBuf>,
    pub vulkan_driver_files: Option<PathBuf>,
    pub mesa_shader_cache_dir: Option<PathBuf>,
    pub identity_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperProtocol {
    OneShotCli,
    ResidentBroker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperTuning {
    pub threads: Option<NonZeroUsize>,
    pub beam_size: Option<u8>,
    pub best_of: Option<u8>,
    pub no_fallback: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WhisperTuningOverride {
    pub threads: Option<NonZeroUsize>,
    pub beam_size: Option<u8>,
    pub best_of: Option<u8>,
    pub no_fallback: Option<bool>,
}

impl WhisperTuningOverride {
    #[must_use]
    pub fn apply(self, defaults: WhisperTuning) -> WhisperTuning {
        WhisperTuning {
            threads: self.threads.or(defaults.threads),
            beam_size: self.beam_size.or(defaults.beam_size),
            best_of: self.best_of.or(defaults.best_of),
            no_fallback: self.no_fallback.or(defaults.no_fallback),
        }
    }
}

impl WhisperTuning {
    #[must_use]
    pub const fn runtime_defaults() -> Self {
        Self {
            threads: None,
            beam_size: None,
            best_of: None,
            no_fallback: None,
        }
    }
}

impl WhisperExecutionPlan {
    #[must_use]
    pub fn one_shot(
        runtime: WhisperRuntimeCandidate,
        model: WhisperModelAsset,
        vad: Option<PathBuf>,
    ) -> Self {
        Self {
            runtime,
            model,
            vad,
            tuning: WhisperTuning::runtime_defaults(),
            protocol: WhisperProtocol::OneShotCli,
            force_cpu: false,
            timeout: Duration::from_secs(15 * 60),
            allow_vad_retry: true,
        }
    }
}

#[must_use]
pub fn preferred_runtime(
    candidates: &[WhisperRuntimeCandidate],
) -> Option<&WhisperRuntimeCandidate> {
    candidates
        .iter()
        .min_by_key(|candidate| match candidate.source {
            WhisperRuntimeSource::Managed => 0,
            WhisperRuntimeSource::System => 1,
            WhisperRuntimeSource::Unknown => 2,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_core::WhisperVulkanReceipt;

    use crate::stt::AdmissionIdentityKey;

    fn candidate(source: WhisperRuntimeSource, name: &str) -> WhisperRuntimeCandidate {
        WhisperRuntimeCandidate {
            source,
            backend: WhisperRuntimeBackend::Cpu,
            cli: PathBuf::from(name),
            server: None,
            launch: WhisperRuntimeLaunch::default(),
        }
    }

    #[test]
    fn managed_cpu_preserves_existing_precedence() {
        let system = candidate(WhisperRuntimeSource::System, "system");
        let managed = candidate(WhisperRuntimeSource::Managed, "managed");
        assert_eq!(
            preferred_runtime(&[system, managed]).map(|runtime| runtime.cli.as_path()),
            Some(std::path::Path::new("managed"))
        );
    }

    #[test]
    fn normal_runs_preserve_runtime_defaults() {
        let tuning = WhisperTuning::runtime_defaults();
        assert_eq!(tuning.threads, None);
        assert_eq!(tuning.beam_size, None);
        assert_eq!(tuning.best_of, None);
        assert_eq!(tuning.no_fallback, None);
        let plan = WhisperExecutionPlan::one_shot(
            candidate(WhisperRuntimeSource::Managed, "managed"),
            WhisperModelAsset {
                name: "base".to_string(),
                path: PathBuf::from("model.bin"),
                multilingual: true,
            },
            None,
        );
        assert!(!plan.force_cpu);
        assert!(plan.allow_vad_retry);
    }

    #[test]
    fn overrides_change_only_the_requested_dimensions() {
        let defaults = WhisperTuning::runtime_defaults();
        let tuning = WhisperTuningOverride {
            beam_size: Some(1),
            no_fallback: Some(true),
            ..WhisperTuningOverride::default()
        }
        .apply(defaults);
        assert_eq!(tuning.threads, None);
        assert_eq!(tuning.beam_size, Some(1));
        assert_eq!(tuning.best_of, None);
        assert_eq!(tuning.no_fallback, Some(true));
    }

    fn plan(source: WhisperRuntimeSource, backend: WhisperRuntimeBackend) -> WhisperExecutionPlan {
        let mut plan = WhisperExecutionPlan::one_shot(
            WhisperRuntimeCandidate {
                source,
                backend,
                cli: PathBuf::from(match source {
                    WhisperRuntimeSource::Managed => "managed",
                    WhisperRuntimeSource::System | WhisperRuntimeSource::Unknown => "accelerated",
                }),
                server: None,
                launch: WhisperRuntimeLaunch {
                    identity_sha256: Some("a".repeat(64)),
                    ..WhisperRuntimeLaunch::default()
                },
            },
            WhisperModelAsset {
                name: "small".to_string(),
                path: PathBuf::from("model.bin"),
                multilingual: true,
            },
            Some(PathBuf::from("vad.bin")),
        );
        plan.tuning = WhisperTuning {
            threads: NonZeroUsize::new(4),
            beam_size: Some(3),
            best_of: Some(5),
            no_fallback: Some(false),
        };
        plan.force_cpu = source == WhisperRuntimeSource::Managed;
        plan.allow_vad_retry = false;
        plan
    }

    fn receipt() -> WhisperVulkanReceipt {
        WhisperVulkanReceipt {
            schema_version: 1,
            backend: "vulkan".to_string(),
            selected_index: 0,
            vendor_id: 0x8086,
            device_id: 0x46a6,
            api_version: 4_211_006,
            driver_version: 104_865_800,
            device_uuid: "1".repeat(32),
            driver_uuid: "2".repeat(32),
            pipeline_cache_uuid: "3".repeat(32),
        }
    }

    #[test]
    fn qualified_plan_requires_exact_accelerator_and_managed_cpu_contracts() {
        let key: AdmissionIdentityKey = serde_json::from_str(&format!("\"{}\"", "a".repeat(64)))
            .unwrap();
        let accelerated = plan(WhisperRuntimeSource::System, WhisperRuntimeBackend::Vulkan);
        let cpu = plan(WhisperRuntimeSource::Managed, WhisperRuntimeBackend::Cpu);
        assert!(WhisperPlanDecision::qualified(
            key.clone(),
            accelerated.clone(),
            cpu.clone(),
            receipt()
        )
        .is_ok());

        let mut changed_model = cpu.clone();
        changed_model.model.name = "base".to_string();
        let mut changed_tuning = cpu.clone();
        changed_tuning.tuning.beam_size = Some(1);
        let wrong_backend = plan(WhisperRuntimeSource::System, WhisperRuntimeBackend::Cpu);
        for (primary, fallback) in [
            (accelerated.clone(), changed_model),
            (accelerated.clone(), changed_tuning),
            (wrong_backend, cpu.clone()),
            (accelerated.clone(), accelerated.clone()),
        ] {
            assert!(WhisperPlanDecision::qualified(
                key.clone(),
                primary,
                fallback,
                receipt()
            )
            .is_err());
        }
    }
}
