use serde::{Deserialize, Serialize};

use crate::dictionary::RecognitionHints;
use crate::language::LanguageChoice;
use crate::types::{EngineId, Pcm16kMono};

/// What actually ran on a transcription, observed from the engine rather than
/// requested in configuration. Every field is optional: Parakeet has no model
/// file or multilingual flag to report, and the fake engine has nothing at all.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunDetail {
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default)]
    pub vad_path: Option<String>,
    #[serde(default)]
    pub multilingual: Option<bool>,
    #[serde(default)]
    pub vad: Option<bool>,
    /// The detected or pinned language the engine reported, with the
    /// detection probability when whisper.cpp ran auto-detection.
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub language_probability: Option<f32>,
    /// Whisper-only timing and attempt detail. Rows written before split
    /// telemetry deserialize with this field absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whisper: Option<WhisperRunTelemetry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperRunTelemetry {
    pub mode: WhisperRunMode,
    pub total_ms: u64,
    pub audio_encode_ms: u64,
    pub parse_ms: u64,
    pub runtime: WhisperRuntimeTelemetry,
    pub tuning: WhisperTuningTelemetry,
    pub attempts: Vec<WhisperAttemptTelemetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<WhisperRecoveryTelemetry>,
    /// Set when the user asked for the GPU and the request never reached it.
    /// Absent on a CPU-by-choice run, so its presence alone means a fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_acceleration: Option<WhisperAccelerationSkip>,
}

/// Why a run the user asked to accelerate was sent to the CPU before it ever
/// started. A closed set: the readout renders copy per variant, never a raw
/// internal message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperAccelerationSkip {
    RuntimeMissing,
    NoDeviceEnumerated,
    PinnedDeviceAbsent,
    DeviceQuarantined,
    /// The accelerated plan needs the managed CPU runtime as the path a failed
    /// GPU run retreats to. A system whisper-cli cannot serve that role, so
    /// with only one installed there is no route to the GPU at all.
    CpuFallbackMissing,
    /// The device enumerated and was offered in the picker, but no verified
    /// plan could be built on it. Distinct from finding no device at all,
    /// which is what this used to be reported as.
    DeviceNotReady,
}

impl WhisperAccelerationSkip {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeMissing => "runtimeMissing",
            Self::NoDeviceEnumerated => "noDeviceEnumerated",
            Self::PinnedDeviceAbsent => "pinnedDeviceAbsent",
            Self::DeviceQuarantined => "deviceQuarantined",
            Self::CpuFallbackMissing => "cpuFallbackMissing",
            Self::DeviceNotReady => "deviceNotReady",
        }
    }
}

/// Which backend the user asked for. There is no automatic mode: a guess the
/// user cannot see is what this setting used to be, and it never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperAccelerationPreference {
    /// Legacy `auto` resolves here. Auto never accelerated anything in a
    /// shipped build, so CPU is what those configs were already getting.
    #[serde(alias = "auto")]
    Cpu,
    Gpu,
}

impl WhisperAccelerationPreference {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "cpu" | "auto" => Some(Self::Cpu),
            "gpu" => Some(Self::Gpu),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperRecoveryTelemetry {
    pub identity_key: String,
    pub accelerated_attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<WhisperRecoveryReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperRecoveryReason {
    Quarantined,
    QuarantineUnreadable,
    RuntimeFailure,
    Timeout,
    MalformedOutput,
    MissingReceipt,
    ReceiptMismatch,
    CpuFallback,
    IdentityMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperTuningTelemetry {
    pub threads: Option<usize>,
    pub beam_size: Option<u8>,
    pub best_of: Option<u8>,
    pub no_fallback: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperRunMode {
    ColdCli,
    ColdFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperRuntimeTelemetry {
    pub binary: String,
    pub source: WhisperRuntimeSource,
    pub backend: WhisperRuntimeBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulkan_driver_files: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesa_shader_cache_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulkan_receipt: Option<WhisperVulkanReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WhisperVulkanReceipt {
    pub schema_version: u32,
    pub backend: String,
    pub selected_index: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub api_version: u32,
    pub driver_version: u32,
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    #[serde(rename = "driverUUID")]
    pub driver_uuid: String,
    #[serde(rename = "pipelineCacheUUID")]
    pub pipeline_cache_uuid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperRuntimeSource {
    Managed,
    System,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperRuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
    OpenVino,
    Rocm,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperAttemptTelemetry {
    pub vad: bool,
    pub process_start_ms: u64,
    pub child_wall_ms: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_reason: Option<WhisperRetryReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhisperRetryReason {
    VadRejected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub raw: String,
    pub engine: EngineId,
    pub audio_ms: u64,
    pub infer_ms: u64,
    pub detail: RunDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeOptions {
    pub language: LanguageChoice,
    pub hints: RecognitionHints,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    Missing,
    Infer(String),
}

impl EngineError {
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Missing => "engine or model missing".to_string(),
            Self::Infer(msg) => msg.clone(),
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl std::error::Error for EngineError {}

pub trait Engine {
    fn id(&self) -> EngineId;
    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_run_detail_json_remains_compatible() {
        let detail: RunDetail =
            serde_json::from_str(r#"{"binary":"whisper-cli","model_path":"model.bin","vad":true}"#)
                .unwrap();
        assert_eq!(detail.binary.as_deref(), Some("whisper-cli"));
        assert!(detail.whisper.is_none());
    }

    #[test]
    fn old_runtime_telemetry_without_device_remains_compatible() {
        let runtime: WhisperRuntimeTelemetry =
            serde_json::from_str(r#"{"binary":"whisper-cli","source":"system","backend":"cpu"}"#)
                .unwrap();
        assert_eq!(runtime.backend, WhisperRuntimeBackend::Cpu);
        assert!(runtime.device.is_none());
        assert!(runtime.library_path.is_none());
        assert!(runtime.identity_sha256.is_none());
    }

    #[test]
    fn legacy_acceleration_preferences_migrate_in_opposite_directions() {
        // Configs written before 0.12.6 hold "gpu" and meant it; configs
        // written after hold "auto", which only ever ran on the CPU.
        for (raw, expected) in [
            ("auto", WhisperAccelerationPreference::Cpu),
            ("cpu", WhisperAccelerationPreference::Cpu),
            ("gpu", WhisperAccelerationPreference::Gpu),
        ] {
            let loaded: WhisperAccelerationPreference =
                serde_json::from_str(&format!("\"{raw}\"")).unwrap();
            assert_eq!(loaded, expected, "{raw}");
            assert_eq!(WhisperAccelerationPreference::parse(raw), Some(expected));
        }
        assert_eq!(WhisperAccelerationPreference::Cpu.as_str(), "cpu");
        assert_eq!(WhisperAccelerationPreference::Gpu.as_str(), "gpu");
    }
}
