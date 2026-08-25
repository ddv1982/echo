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
}
