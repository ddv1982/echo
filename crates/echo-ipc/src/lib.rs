use std::collections::BTreeSet;

use ts_rs::{Config, TS};

#[rustfmt::skip]
mod schema {
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub phase: AppPhase,
    pub last_transcript: Option<String>,
    pub last_history_id: Option<String>,
    pub microphone_ready: bool,
    pub engine_name: String,
    pub engine_ready: bool,
    pub injection_name: String,
    pub injection_ready: bool,
    pub shortcut: ShortcutStatus,
    pub hud_enabled: bool,
    pub recording_limit_seconds: Option<u32>,
    pub recording_policy: RecordingPolicy,
    pub settings_path: String,
    pub version: String,
    pub last_error: Option<String>,
    pub last_run: Option<LastRun>,
    pub language_warning: Option<String>,
    pub recording_in_process: bool,
    pub current_exe: String,
    pub first_path_hit: Option<String>,
    pub stale_installs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub enum AppPhase {
    Idle,
    Recording,
    Transcribing,
    Injecting,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPolicy {
    pub minimum_seconds: u32,
    pub default_seconds: u32,
    pub maximum_seconds: u32,
    pub presets_seconds: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ShortcutStatus {
    Probing {
        desired: String,
    },
    Active {
        desired: String,
        effective: String,
        backend: ShortcutBackend,
        activation: Option<String>,
        verification_identity: String,
    },
    GnomeReady {
        desired: String,
        effective: String,
        detail: String,
        command: String,
        binding: String,
        activation: Option<String>,
        verification_identity: String,
    },
    GnomeSetup {
        desired: String,
        setup: GnomeShortcutSetup,
    },
    Manual {
        desired: String,
        command: String,
        detail: String,
    },
    Failed {
        desired: String,
        detail: String,
    },
    Unsupported {
        desired: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutBackend {
    Portal,
    X11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GnomeShortcutState {
    Missing,
    Stale,
    Conflicting,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GnomeShortcutSetup {
    pub state: GnomeShortcutState,
    pub detail: String,
    pub command: String,
    pub binding: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum LegacyShortcutState {
    Missing,
    Stale,
    Conflicting,
    Ready,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LegacyShortcutSetup {
    pub state: LegacyShortcutState,
    pub detail: String,
    pub command: String,
    pub binding: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub engine: String,
    pub binary: Option<String>,
    pub model_path: Option<String>,
    pub multilingual: Option<bool>,
    pub vad: Option<bool>,
    pub infer_ms: u64,
    pub language: Option<String>,
    pub language_probability: Option<f32>,
    pub performance: Option<LastRunPerformance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LastRunPerformance {
    pub mode: RunMode,
    pub runtime_source: RuntimeSource,
    pub backend: RuntimeBackend,
    pub device: Option<String>,
    pub total_ms: u64,
    pub audio_encode_ms: u64,
    pub child_wall_ms: u64,
    pub parse_ms: u64,
    pub attempt_count: usize,
    pub tuning: TuningTelemetry,
    pub acceleration_skip: Option<AccelerationSkipReason>,
    pub recovery: Option<RecoveryTelemetry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunMode {
    ColdCli,
    ColdFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSource {
    Managed,
    System,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
    OpenVino,
    Rocm,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TuningTelemetry {
    pub threads: Option<usize>,
    pub beam_size: Option<u8>,
    pub best_of: Option<u8>,
    pub no_fallback: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryTelemetry {
    pub identity_key: String,
    pub accelerated_attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<RecoveryReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryReason {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccelerationSkipReason {
    RuntimeMissing,
    NoDeviceEnumerated,
    PinnedDeviceAbsent,
    DeviceQuarantined,
    CpuFallbackMissing,
    DeviceNotReady,
    RecoveredToCpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SettingSource {
    Env,
    File,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SettingField<T> {
    pub value: Option<T>,
    pub effective: T,
    pub source: SettingSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub engine: SettingField<String>,
    pub whisper_model: SettingField<String>,
    pub hud: SettingField<bool>,
    pub record_seconds: SettingField<u32>,
    pub language: SettingField<String>,
    pub whisper_acceleration: SettingField<String>,
    pub whisper_gpu_device: SettingField<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SettingsChange {
    Engine { value: Option<String> },
    WhisperModel { value: Option<String> },
    Hud { value: Option<bool> },
    RecordSeconds { value: Option<u32> },
    Language { value: Option<String> },
    WhisperAcceleration { value: Option<String> },
    WhisperGpuDevice { value: Option<String> },
    EnableWhisperGpu,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub preferences: Settings,
    pub transcription: TranscriptionSnapshot,
    pub readiness: Readiness,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSnapshot {
    pub next_run: NextSpeechRun,
    pub languages: LanguageOptions,
    pub models: ModelInventory,
    pub whisper: WhisperApplicability,
    pub last_used: Option<LastRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum NextSpeechRun {
    Ready {
        engine: ResolvedSpeechEngine,
        language: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ResolvedSpeechEngine {
    Whisper { model: String, multilingual: bool },
    Parakeet { model: String },
    Fake,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum WhisperApplicability {
    Applicable { gpu: WhisperGpuSetup },
    Deferred { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum WhisperGpuSetup {
    NotRequested,
    Ready,
    NeedsInstall { component: ComponentStatus },
    Unsupported { component: ComponentStatus },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub text: String,
    pub raw: String,
    pub engine: String,
    pub started_at: u64,
    pub infer_ms: u64,
    pub injection: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryBatchResult {
    pub entries: Vec<DictionaryItem>,
    pub added: usize,
    pub unchanged: usize,
    pub conflicts: Vec<DictionaryConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryConflict {
    pub spoken: String,
    pub written: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryItem {
    pub spoken: String,
    pub written: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryTrainingSample {
    pub transcript: String,
    pub engine: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LanguageMode {
    Multilingual,
    English,
    Parakeet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LanguageGroup {
    Common,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOption {
    pub code: String,
    pub english_name: String,
    pub group: LanguageGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOptions {
    pub mode: LanguageMode,
    pub model: Option<String>,
    pub options: Vec<LanguageOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelInfo {
    pub name: String,
    pub path: String,
    pub family: String,
    pub multilingual: bool,
    pub quantisation: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EngineAvailability {
    pub id: String,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelInventory {
    pub whisper: Vec<WhisperModelInfo>,
    pub vad: Vec<String>,
    pub parakeet: Option<String>,
    pub engines: Vec<EngineAvailability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VulkanDeviceId {
    #[serde(rename = "deviceUUID")]
    pub device_uuid: String,
    #[serde(rename = "driverUUID")]
    pub driver_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub id: VulkanDeviceId,
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub drm_driver: Option<String>,
    pub software: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AudioHost {
    PipeWire,
    PulseAudio,
    Alsa,
    CoreAudio,
    Wasapi,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InputTransport {
    Bluetooth,
    Usb,
    BuiltIn,
    Pci,
    Network,
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum EndpointTier {
    Primary,
    Advanced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub label: String,
    pub is_default: bool,
    pub manufacturer: Option<String>,
    pub device_type: Option<String>,
    pub interface_type: Option<String>,
    pub address: Option<String>,
    pub driver: Option<String>,
    pub extended: Vec<String>,
    pub host: AudioHost,
    pub transport: InputTransport,
    pub tier: EndpointTier,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MicrophoneSelection {
    SystemDefault {
        active: Option<InputDevice>,
    },
    Selected {
        device: InputDevice,
    },
    LegacyMatch {
        name: String,
        device: InputDevice,
    },
    MissingWithFallback {
        requested_id: String,
        requested_label: String,
        fallback: InputDevice,
    },
    MissingWithoutFallback {
        requested_id: String,
        requested_label: String,
    },
    AmbiguousLegacyName {
        name: String,
        matches: Vec<InputDevice>,
        fallback: Option<InputDevice>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MicrophoneSource {
    Environment,
    Config,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneSnapshot {
    pub host: AudioHost,
    pub source: MicrophoneSource,
    pub system_default: Option<InputDevice>,
    pub system_default_is_proxy: bool,
    pub devices: Vec<InputDevice>,
    pub selection: MicrophoneSelection,
    pub enumeration_warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneTestOutcome {
    Heard,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneFailure {
    Disconnected,
    Selection,
    Permission,
    Busy,
    Unsupported,
    Host,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MicrophoneTestResult {
    Completed {
        device: InputDevice,
        peak_rms: f32,
        dropped_samples: u64,
        outcome: MicrophoneTestOutcome,
    },
    Failed {
        device: Option<InputDevice>,
        category: MicrophoneFailure,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    WhisperRuntime,
    WhisperVulkanRuntime,
    WhisperBaseQ51,
    WhisperSmall,
    WhisperLargeV3TurboQ50,
    SileroVad,
    SherpaRuntime,
    ParakeetTdt06bV3Int8,
}

impl<'de> Deserialize<'de> for ComponentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "whisper-runtime" => Ok(Self::WhisperRuntime),
            "whisper-vulkan-runtime" => Ok(Self::WhisperVulkanRuntime),
            "whisper-base-q51" | "whisper-base-q5-1" => Ok(Self::WhisperBaseQ51),
            "whisper-small" => Ok(Self::WhisperSmall),
            "whisper-large-v3-turbo-q50" | "whisper-large-v3-turbo-q5-0" => {
                Ok(Self::WhisperLargeV3TurboQ50)
            }
            "silero-vad" => Ok(Self::SileroVad),
            "sherpa-runtime" => Ok(Self::SherpaRuntime),
            "parakeet-tdt06b-v3-int8" | "parakeet-tdt-06b-v3-int8" => {
                Ok(Self::ParakeetTdt06bV3Int8)
            }
            _ => Err(serde::de::Error::unknown_variant(
                &value,
                &[
                    "whisper-runtime",
                    "whisper-vulkan-runtime",
                    "whisper-base-q51",
                    "whisper-small",
                    "whisper-large-v3-turbo-q50",
                    "silero-vad",
                    "sherpa-runtime",
                    "parakeet-tdt06b-v3-int8",
                ],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SetupPlanId {
    Recommended,
    Parakeet,
    WhisperBase,
    WhisperSmall,
    WhisperLargeV3Turbo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ManagedComponentState {
    Absent {
        resumable_bytes: u64,
    },
    Ready {
        version: String,
        bytes: u64,
        root: String,
    },
    NeedsRepair {
        reason: String,
        resumable_bytes: u64,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InstallPhase {
    CheckingDisk,
    Downloading,
    Verifying,
    Extracting,
    Activating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub operation_id: String,
    pub component: ComponentId,
    pub phase: InstallPhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub resumed_from_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ComponentOrigin {
    System,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ActiveComponentOrigin {
    Managed,
    System,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponent {
    pub origin: ComponentOrigin,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatus {
    pub id: ComponentId,
    pub label: String,
    pub managed: ManagedComponentState,
    pub external: Vec<ExternalComponent>,
    pub active_origin: Option<ActiveComponentOrigin>,
    pub activity: Option<InstallProgress>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetupPlan {
    pub id: SetupPlanId,
    pub label: String,
    pub components: Vec<ComponentId>,
    pub satisfied: bool,
    pub download_bytes: u64,
    pub required_free_bytes: u64,
    pub available_bytes: Option<u64>,
    pub disk_ready: bool,
    pub disk_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub managed_supported: bool,
    pub unsupported_reason: Option<String>,
    pub total_memory_bytes: Option<u64>,
    pub recommended_model: ComponentId,
    pub components: Vec<ComponentStatus>,
    pub plans: Vec<SetupPlan>,
    pub microphone_ready: bool,
    pub speech_ready: bool,
    pub has_successful_dictation: bool,
    pub first_run_complete: bool,
    pub active_operation: Option<String>,
    pub active_cancellable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SetupEvent {
    Progress { progress: InstallProgress },
    Finished { operation_id: String },
    Cancelled { operation_id: String },
    Failed { operation_id: String, error: String },
}

}

macro_rules! schema_types {
    ($callback:ident $($prefix:tt)*) => {
        $callback! {
            $($prefix)*
            schema::AccelerationSkipReason => schema::AccelerationSkipReason,
            schema::ActiveComponentOrigin => schema::ActiveComponentOrigin,
            schema::AppPhase => schema::AppPhase,
            schema::AppStatus => schema::AppStatus,
            schema::AudioHost => schema::AudioHost,
            schema::ComponentId => schema::ComponentId,
            schema::ComponentOrigin => schema::ComponentOrigin,
            schema::ComponentStatus => schema::ComponentStatus,
            schema::DictionaryBatchResult => schema::DictionaryBatchResult,
            schema::DictionaryConflict => schema::DictionaryConflict,
            schema::DictionaryItem => schema::DictionaryItem,
            schema::DictionaryTrainingSample => schema::DictionaryTrainingSample,
            schema::EndpointTier => schema::EndpointTier,
            schema::EngineAvailability => schema::EngineAvailability,
            schema::ExternalComponent => schema::ExternalComponent,
            schema::GnomeShortcutSetup => schema::GnomeShortcutSetup,
            schema::GnomeShortcutState => schema::GnomeShortcutState,
            schema::GpuDevice => schema::GpuDevice,
            schema::HistoryItem => schema::HistoryItem,
            schema::InputDevice => schema::InputDevice,
            schema::InputTransport => schema::InputTransport,
            schema::InstallPhase => schema::InstallPhase,
            schema::InstallProgress => schema::InstallProgress,
            schema::LanguageGroup => schema::LanguageGroup,
            schema::LanguageMode => schema::LanguageMode,
            schema::LanguageOption => schema::LanguageOption,
            schema::LanguageOptions => schema::LanguageOptions,
            schema::LastRun => schema::LastRun,
            schema::LastRunPerformance => schema::LastRunPerformance,
            schema::LegacyShortcutSetup => schema::LegacyShortcutSetup,
            schema::LegacyShortcutState => schema::LegacyShortcutState,
            schema::ManagedComponentState => schema::ManagedComponentState,
            schema::MicrophoneFailure => schema::MicrophoneFailure,
            schema::MicrophoneSelection => schema::MicrophoneSelection,
            schema::MicrophoneSnapshot => schema::MicrophoneSnapshot,
            schema::MicrophoneSource => schema::MicrophoneSource,
            schema::MicrophoneTestOutcome => schema::MicrophoneTestOutcome,
            schema::MicrophoneTestResult => schema::MicrophoneTestResult,
            schema::ModelInventory => schema::ModelInventory,
            schema::NextSpeechRun => schema::NextSpeechRun,
            schema::Readiness => schema::Readiness,
            schema::RecordingPolicy => schema::RecordingPolicy,
            schema::RecoveryReason => schema::RecoveryReason,
            schema::RecoveryTelemetry => schema::RecoveryTelemetry,
            schema::ResolvedSpeechEngine => schema::ResolvedSpeechEngine,
            schema::RunMode => schema::RunMode,
            schema::RuntimeBackend => schema::RuntimeBackend,
            schema::RuntimeSource => schema::RuntimeSource,
            schema::SettingField => schema::SettingField<String>,
            schema::SettingSource => schema::SettingSource,
            schema::Settings => schema::Settings,
            schema::SettingsChange => schema::SettingsChange,
            schema::SettingsSnapshot => schema::SettingsSnapshot,
            schema::SetupEvent => schema::SetupEvent,
            schema::SetupPlan => schema::SetupPlan,
            schema::SetupPlanId => schema::SetupPlanId,
            schema::ShortcutBackend => schema::ShortcutBackend,
            schema::ShortcutStatus => schema::ShortcutStatus,
            schema::TranscriptionSnapshot => schema::TranscriptionSnapshot,
            schema::TuningTelemetry => schema::TuningTelemetry,
            schema::VulkanDeviceId => schema::VulkanDeviceId,
            schema::WhisperApplicability => schema::WhisperApplicability,
            schema::WhisperGpuSetup => schema::WhisperGpuSetup,
            schema::WhisperModelInfo => schema::WhisperModelInfo,
        }
    };
}

macro_rules! export_schema_types {
    ($($export:path => $ty:ty),+ $(,)?) => {
        $(pub use $export;)+
    };
}

schema_types!(export_schema_types);

macro_rules! declarations {
    ($config:expr, $($ty:ty),+ $(,)?) => {{
        let mut output = String::from("// Generated from crates/echo-ipc/src/lib.rs. Do not edit.\n\n");
        let names = [$(<$ty>::name($config)),+];
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "IPC type registry must stay sorted");
        $(
            output.push_str("export ");
            output.push_str(&<$ty>::decl($config));
            output.push_str("\n\n");
        )+
        output.pop();
        let schema_names = names
            .into_iter()
            .map(|name| name.split('<').next().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(output.matches("export type ").count(), schema_names.len());
        (output, schema_names)
    }};
}

fn contract_parts() -> (String, BTreeSet<String>) {
    let config = Config::default().with_large_int("number");
    macro_rules! declare_schema_types {
        ($config:expr; $($export:path => $ty:ty),+ $(,)?) => {
            declarations!($config, $($ty),+)
        };
    }
    schema_types!(declare_schema_types &config;)
}

#[must_use]
pub fn typescript_contract() -> String {
    contract_parts().0
}

#[must_use]
pub fn registered_type_names() -> BTreeSet<String> {
    contract_parts().1
}

#[cfg(feature = "desktop")]
mod projections;
#[cfg(test)]
mod tests;
