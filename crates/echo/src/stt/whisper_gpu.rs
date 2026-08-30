use std::time::{Duration, Instant};

use echo_core::{WhisperRuntimeBackend, WhisperRuntimeSource};
use sha2::{Digest, Sha256};

use super::backend::vulkan::{LocalVulkanRoute, VulkanBackend};
use super::whisper_plan::{
    WhisperExecutionPlan, WhisperPlanDecision, WhisperRuntimeCandidate, WhisperTuning,
};
use super::whisper_quarantine::{AcceleratorKey, QuarantineStore};
use super::whisper_recovery::RecoveringWhisperEngine;
use super::whisper_runtime_launch;

/// How long the first request may spend discovering and probing devices before
/// it gives up and transcribes on the CPU.
const FOREGROUND_DEADLINE: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Why a run the user asked to accelerate did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuSkipReason {
    RuntimeMissing,
    NoDeviceEnumerated,
    PinnedDeviceAbsent,
    DeviceQuarantined,
}

impl GpuSkipReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeMissing => "runtimeMissing",
            Self::NoDeviceEnumerated => "noDeviceEnumerated",
            Self::PinnedDeviceAbsent => "pinnedDeviceAbsent",
            Self::DeviceQuarantined => "deviceQuarantined",
        }
    }
}

/// Beam 3, best-of 5, temperature fallback enabled. This is the configuration
/// with 400 transcriptions of zero WER delta across five languages and a 57.8
/// percent paired median reduction, recorded in
/// `.audit/whisper-phase5-small-v192-b3/decision.md`. It is applied to the
/// accelerated plan and to its CPU fallback so recovery is not a downgrade.
#[must_use]
pub(crate) fn qualified_tuning() -> WhisperTuning {
    WhisperTuning {
        threads: None,
        beam_size: Some(3),
        best_of: Some(5),
        no_fallback: Some(false),
    }
}

/// A device is identified by the receipt fields that survive a reboot plus the
/// driver files behind it, so a Mesa upgrade retires the key on its own.
fn accelerator_key(route: &LocalVulkanRoute) -> Result<AcceleratorKey, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"echo-whisper-accelerator-v1\0");
    for part in [
        route.receipt.device_uuid.as_str(),
        route.receipt.driver_uuid.as_str(),
        route.receipt.pipeline_cache_uuid.as_str(),
        route.fingerprint.icd_manifest_sha256.as_str(),
        route.fingerprint.icd_library_sha256.as_str(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    AcceleratorKey::parse(format!("{:x}", hasher.finalize()))
}

/// Resolve the user's GPU choice into an engine, or say why it stayed on CPU.
/// `pinned` is a `deviceUUID:driverUUID` pair, or `None` for automatic.
pub(crate) fn accelerated_engine(
    managed_cpu: &WhisperExecutionPlan,
    pinned: Option<&str>,
) -> Result<RecoveringWhisperEngine, GpuSkipReason> {
    let runtime = super::installed_vulkan_runtime().ok_or(GpuSkipReason::RuntimeMissing)?;
    let cli = runtime.join("whisper-cli");
    let probe = runtime.join("echo-whisper-runtime-probe");
    if !probe.is_file() {
        return Err(GpuSkipReason::RuntimeMissing);
    }
    let backend = VulkanBackend::bounded(
        probe,
        whisper_runtime_launch(&cli),
        PROBE_TIMEOUT,
        Instant::now() + FOREGROUND_DEADLINE,
    );
    let routes = backend
        .enumerate()
        .map_err(|_| GpuSkipReason::NoDeviceEnumerated)?;
    let route = select_route(&routes, pinned)?;

    let key = accelerator_key(route).map_err(|_| GpuSkipReason::NoDeviceEnumerated)?;
    let quarantine = QuarantineStore::at(super::whisper_state_dir().join("gpu-quarantine.json"));
    if quarantine.is_active(&key, unix_time()).unwrap_or(true) {
        return Err(GpuSkipReason::DeviceQuarantined);
    }
    let ready = backend
        .ready(route)
        .map_err(|_| GpuSkipReason::NoDeviceEnumerated)?;

    let mut launch = whisper_runtime_launch(&cli);
    launch.vulkan_driver_files = Some(route.manifest_path.clone());
    launch.vulkan_selector = Some(route.selector.clone());
    launch.mesa_shader_cache_dir =
        Some(super::whisper_state_dir().join("mesa").join(key.as_str()));

    let tuning = qualified_tuning();
    let mut primary = WhisperExecutionPlan::one_shot(
        WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Vulkan,
            cli,
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

    let decision = WhisperPlanDecision::qualified(key, primary, fallback, ready)
        .map_err(|_| GpuSkipReason::NoDeviceEnumerated)?;
    Ok(RecoveringWhisperEngine::new(decision, quarantine))
}

/// A pin selects that device or nothing. Silently running on a different card
/// than the one chosen is the hidden decision this control exists to remove.
fn select_route<'a>(
    routes: &'a [LocalVulkanRoute],
    pinned: Option<&str>,
) -> Result<&'a LocalVulkanRoute, GpuSkipReason> {
    if let Some(pinned) = pinned.filter(|value| !value.is_empty()) {
        return routes
            .iter()
            .find(|route| {
                let device = route.device();
                format!("{}:{}", device.id.device_uuid, device.id.driver_uuid) == pinned
            })
            .ok_or(GpuSkipReason::PinnedDeviceAbsent);
    }
    routes
        .iter()
        .find(|route| !route.software)
        .ok_or(GpuSkipReason::NoDeviceEnumerated)
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_tuning_matches_the_measured_configuration() {
        let tuning = qualified_tuning();
        assert_eq!(tuning.beam_size, Some(3));
        assert_eq!(tuning.best_of, Some(5));
        assert_eq!(
            tuning.no_fallback,
            Some(false),
            "temperature fallback stays enabled"
        );
    }
}
