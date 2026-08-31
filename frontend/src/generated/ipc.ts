// Generated from crates/echo-ipc/src/lib.rs. Do not edit.

export type AccelerationSkipReason = "runtimeMissing" | "noDeviceEnumerated" | "pinnedDeviceAbsent" | "deviceQuarantined" | "cpuFallbackMissing" | "deviceNotReady" | "recoveredToCpu";

export type ActiveComponentOrigin = "managed" | "system" | "external";

export type AppStatus = { phase: string, lastTranscript: string | null, recording: boolean, microphoneReady: boolean, engineName: string, engineReady: boolean, injectionName: string, injectionReady: boolean, shortcut: ShortcutStatus, hudEnabled: boolean, recordingLimitSeconds: number | null, recordingPolicy: RecordingPolicy, settingsPath: string, version: string, lastError: string | null, lastRun: LastRun | null, languageWarning: string | null, recordingInProcess: boolean, currentExe: string, firstPathHit: string | null, staleInstalls: Array<string>, };

export type AudioHost = "pipe-wire" | "pulse-audio" | "alsa" | "core-audio" | "wasapi" | "other";

export type ComponentId = "whisper-runtime" | "whisper-vulkan-runtime" | "whisper-base-q51" | "whisper-small" | "whisper-large-v3-turbo-q50" | "silero-vad" | "sherpa-runtime" | "parakeet-tdt06b-v3-int8";

export type ComponentOrigin = "system" | "external";

export type ComponentStatus = { id: ComponentId, label: string, managed: ManagedComponentState, external: Array<ExternalComponent>, activeOrigin: ActiveComponentOrigin | null, activity: InstallProgress | null, };

export type DictionaryBatchResult = { entries: Array<DictionaryItem>, added: number, unchanged: number, conflicts: Array<DictionaryConflict>, };

export type DictionaryConflict = { spoken: string, written: string, };

export type DictionaryItem = { spoken: string, written: string, createdAt: number, };

export type DictionaryTrainingSample = { transcript: string, engine: string, };

export type EndpointTier = "primary" | "advanced";

export type EngineAvailability = { id: string, available: boolean, reason: string | null, };

export type ExternalComponent = { origin: ComponentOrigin, path: string, };

export type GnomeShortcutSetup = { state: GnomeShortcutState, detail: string, command: string, binding: string, };

export type GnomeShortcutState = "missing" | "stale" | "conflicting" | "unsupported";

export type GpuDevice = { id: VulkanDeviceId, name: string, vendorId: number, deviceId: number, drmDriver: string | null, software: boolean, };

export type HistoryItem = { id: string, text: string, raw: string, engine: string, startedAt: number, inferMs: number, injection: string, };

export type InputDevice = { id: string, label: string, isDefault: boolean, manufacturer: string | null, deviceType: string | null, interfaceType: string | null, address: string | null, driver: string | null, extended: Array<string>, host: AudioHost, transport: InputTransport, tier: EndpointTier, hint: string, };

export type InputTransport = "bluetooth" | "usb" | "built-in" | "pci" | "network" | "virtual" | "unknown";

export type InstallPhase = "checking-disk" | "downloading" | "verifying" | "extracting" | "activating";

export type InstallProgress = { operationId: string, component: ComponentId, phase: InstallPhase, receivedBytes: number, totalBytes: number, resumedFromBytes: number, };

export type LanguageGroup = "common" | "all";

export type LanguageMode = "multilingual" | "english" | "parakeet";

export type LanguageOption = { code: string, englishName: string, group: LanguageGroup, };

export type LanguageOptions = { mode: LanguageMode, model: string | null, options: Array<LanguageOption>, };

export type LastRun = { engine: string, binary: string | null, modelPath: string | null, multilingual: boolean | null, vad: boolean | null, inferMs: number, language: string | null, languageProbability: number | null, performance: LastRunPerformance | null, };

export type LastRunPerformance = { mode: RunMode, runtimeSource: RuntimeSource, backend: RuntimeBackend, device: string | null, totalMs: number, audioEncodeMs: number, childWallMs: number, parseMs: number, attemptCount: number, tuning: TuningTelemetry, accelerationSkip: AccelerationSkipReason | null, recovery: RecoveryTelemetry | null, };

export type LegacyShortcutSetup = { state: LegacyShortcutState, detail: string, command: string, binding: string, };

export type LegacyShortcutState = "missing" | "stale" | "conflicting" | "ready" | "unsupported";

export type ManagedComponentState = { "kind": "absent", resumableBytes: number, } | { "kind": "ready", version: string, bytes: number, root: string, } | { "kind": "needs-repair", reason: string, resumableBytes: number, } | { "kind": "unsupported", reason: string, };

export type MicrophoneFailure = "disconnected" | "selection" | "permission" | "busy" | "unsupported" | "host" | "failed";

export type MicrophoneSelection = { "kind": "system-default", active: InputDevice | null, } | { "kind": "selected", device: InputDevice, } | { "kind": "legacy-match", name: string, device: InputDevice, } | { "kind": "missing-with-fallback", requestedId: string, requestedLabel: string, fallback: InputDevice, } | { "kind": "missing-without-fallback", requestedId: string, requestedLabel: string, } | { "kind": "ambiguous-legacy-name", name: string, matches: Array<InputDevice>, fallback: InputDevice | null, };

export type MicrophoneSnapshot = { host: AudioHost, source: MicrophoneSource, systemDefault: InputDevice | null, systemDefaultIsProxy: boolean, devices: Array<InputDevice>, selection: MicrophoneSelection, enumerationWarning: string | null, };

export type MicrophoneSource = "environment" | "config" | "default";

export type MicrophoneTestOutcome = "heard" | "silent";

export type MicrophoneTestResult = { "kind": "completed", device: InputDevice, peakRms: number, outcome: MicrophoneTestOutcome, } | { "kind": "failed", device: InputDevice | null, category: MicrophoneFailure, message: string, };

export type ModelInventory = { whisper: Array<WhisperModelInfo>, vad: Array<string>, parakeet: string | null, engines: Array<EngineAvailability>, };

export type NextSpeechRun = { "kind": "ready", engine: ResolvedSpeechEngine, language: string, } | { "kind": "unavailable", reason: string, };

export type Readiness = { managedSupported: boolean, unsupportedReason: string | null, totalMemoryBytes: number | null, recommendedModel: ComponentId, components: Array<ComponentStatus>, plans: Array<SetupPlan>, microphoneReady: boolean, speechReady: boolean, hasSuccessfulDictation: boolean, firstRunComplete: boolean, activeOperation: string | null, activeCancellable: boolean, };

export type RecordingPolicy = { minimumSeconds: number, defaultSeconds: number, maximumSeconds: number, presetsSeconds: Array<number>, };

export type RecoveryReason = "quarantined" | "quarantineUnreadable" | "runtimeFailure" | "timeout" | "malformedOutput" | "missingReceipt" | "receiptMismatch" | "cpuFallback" | "identityMismatch";

export type RecoveryTelemetry = { identityKey: string, acceleratedAttempted: boolean, fallbackReason?: RecoveryReason | null, };

export type ResolvedSpeechEngine = { "kind": "whisper", model: string, multilingual: boolean, } | { "kind": "parakeet", model: string, } | { "kind": "fake" };

export type RunMode = "coldCli" | "coldFallback";

export type RuntimeBackend = "cpu" | "cuda" | "vulkan" | "openVino" | "rocm" | "unknown";

export type RuntimeSource = "managed" | "system" | "unknown";

export type SettingField<T> = { value: T | null, effective: T, source: SettingSource, };

export type SettingSource = "env" | "file" | "default";

export type Settings = { engine: SettingField<string>, whisperModel: SettingField<string>, hud: SettingField<boolean>, recordSeconds: SettingField<number>, language: SettingField<string>, whisperAcceleration: SettingField<string>, whisperGpuDevice: SettingField<string>, };

export type SettingsChange = { "kind": "engine", value: string | null, } | { "kind": "whisperModel", value: string | null, } | { "kind": "hud", value: boolean | null, } | { "kind": "recordSeconds", value: number | null, } | { "kind": "language", value: string | null, } | { "kind": "whisperAcceleration", value: string | null, } | { "kind": "whisperGpuDevice", value: string | null, } | { "kind": "enableWhisperGpu" };

export type SettingsSnapshot = { preferences: Settings, transcription: TranscriptionSnapshot, readiness: Readiness, };

export type SetupEvent = { "kind": "progress", progress: InstallProgress, } | { "kind": "finished", operationId: string, } | { "kind": "cancelled", operationId: string, } | { "kind": "failed", operationId: string, error: string, };

export type SetupPlan = { id: SetupPlanId, label: string, components: Array<ComponentId>, satisfied: boolean, downloadBytes: number, requiredFreeBytes: number, availableBytes: number | null, diskReady: boolean, diskReason: string | null, };

export type SetupPlanId = "recommended" | "parakeet" | "whisper-base" | "whisper-small" | "whisper-large-v3-turbo";

export type ShortcutBackend = "portal" | "x11";

export type ShortcutStatus = { "kind": "probing", desired: string, } | { "kind": "active", desired: string, effective: string, backend: ShortcutBackend, activation: string | null, verificationIdentity: string, } | { "kind": "gnome-ready", desired: string, effective: string, detail: string, command: string, binding: string, activation: string | null, verificationIdentity: string, } | { "kind": "gnome-setup", desired: string, setup: GnomeShortcutSetup, } | { "kind": "manual", desired: string, command: string, detail: string, } | { "kind": "failed", desired: string, detail: string, } | { "kind": "unsupported", desired: string, detail: string, };

export type TranscriptionSnapshot = { nextRun: NextSpeechRun, languages: LanguageOptions, models: ModelInventory, whisper: WhisperApplicability, lastUsed: LastRun | null, };

export type TuningTelemetry = { threads: number | null, beamSize: number | null, bestOf: number | null, noFallback: boolean | null, };

export type VulkanDeviceId = { deviceUUID: string, driverUUID: string, };

export type WhisperApplicability = { "kind": "applicable", gpu: WhisperGpuSetup, } | { "kind": "deferred", reason: string, };

export type WhisperGpuSetup = { "kind": "not-requested" } | { "kind": "ready" } | { "kind": "needs-install", component: ComponentStatus, } | { "kind": "unsupported", component: ComponentStatus, };

export type WhisperModelInfo = { name: string, path: string, family: string, multilingual: boolean, quantisation: string | null, sizeBytes: number, };
