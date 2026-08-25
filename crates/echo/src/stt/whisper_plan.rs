use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use echo_core::{WhisperRuntimeBackend, WhisperRuntimeSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperExecutionPlan {
    pub runtime: WhisperRuntimeCandidate,
    pub model: WhisperModelAsset,
    pub vad: Option<PathBuf>,
    pub tuning: WhisperTuning,
    pub protocol: WhisperProtocol,
    pub force_cpu: bool,
    pub timeout: Duration,
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
}
