export type View = 'home' | 'history' | 'dictionary' | 'settings'
export type ThemeMode = 'system' | 'light' | 'dark'
export type SettingSource = 'env' | 'file' | 'default'
export type MicrophoneSource = 'environment' | 'config' | 'default'
export type AudioHost = 'pipe-wire' | 'pulse-audio' | 'alsa' | 'core-audio' | 'wasapi' | 'other'
export type InputTransport = 'bluetooth' | 'usb' | 'built-in' | 'pci' | 'network' | 'virtual' | 'unknown'
export type EndpointTier = 'primary' | 'advanced'

export interface SettingField<T> {
  value: T | null
  effective: T
  source: SettingSource
}

export interface Settings {
  engine: SettingField<string>
  whisperModel: SettingField<string>
  cleanup: SettingField<string>
  hud: SettingField<boolean>
  recordSeconds: SettingField<number>
  language: SettingField<string>
}

export interface RecordingPolicy {
  minimumSeconds: number
  defaultSeconds: number
  maximumSeconds: number
  presetsSeconds: number[]
}

export interface InputDevice {
  id: string
  label: string
  isDefault: boolean
  manufacturer: string | null
  deviceType: string | null
  interfaceType: string | null
  address: string | null
  driver: string | null
  extended: string[]
  host: AudioHost
  transport: InputTransport
  tier: EndpointTier
  hint: string
}

export type MicrophoneSelection =
  | { kind: 'system-default'; active: InputDevice | null }
  | { kind: 'selected'; device: InputDevice }
  | { kind: 'legacy-match'; name: string; device: InputDevice }
  | {
      kind: 'missing-with-fallback'
      requestedId: string
      requestedLabel: string
      fallback: InputDevice
    }
  | { kind: 'missing-without-fallback'; requestedId: string; requestedLabel: string }
  | {
      kind: 'ambiguous-legacy-name'
      name: string
      matches: InputDevice[]
      fallback: InputDevice | null
    }

export interface MicrophoneSnapshot {
  host: AudioHost
  source: MicrophoneSource
  systemDefault: InputDevice | null
  systemDefaultIsProxy: boolean
  devices: InputDevice[]
  selection: MicrophoneSelection
  enumerationWarning: string | null
}

export type MicrophoneTestResult =
  | {
      kind: 'completed'
      device: InputDevice
      peakRms: number
      outcome: 'heard' | 'silent'
    }
  | {
      kind: 'failed'
      device: InputDevice | null
      category:
        | 'disconnected'
        | 'selection'
        | 'permission'
        | 'busy'
        | 'unsupported'
        | 'host'
        | 'failed'
      message: string
    }

export interface AppStatus {
  phase: string
  lastTranscript: string | null
  recording: boolean
  microphoneReady: boolean
  engineName: string
  engineReady: boolean
  injectionName: string
  injectionReady: boolean
  shortcut: ShortcutStatus
  cleanupName: string
  hudEnabled: boolean
  recordingLimitSeconds: number | null
  recordingPolicy: RecordingPolicy
  settingsPath: string
  version: string
  lastError: string | null
  lastRun: LastRun | null
  languageWarning: string | null
  recordingInProcess: boolean
  currentExe: string
  firstPathHit: string | null
  staleInstalls: string[]
}

export type ShortcutStatus =
  | { kind: 'probing'; desired: string }
  | {
      kind: 'active'
      desired: string
      effective: string
      backend: 'portal' | 'x11'
      activation: string | null
      verificationIdentity: string
    }
  | {
      kind: 'gnome-ready'
      desired: string
      effective: string
      detail: string
      command: string
      binding: string
      activation: string | null
      verificationIdentity: string
    }
  | { kind: 'gnome-setup'; desired: string; setup: GnomeShortcutSetup }
  | {
      kind: 'manual'
      desired: string
      command: string
      detail: string
    }
  | { kind: 'failed'; desired: string; detail: string }
  | { kind: 'unsupported'; desired: string; detail: string }

interface LegacyShortcutFields {
  detail: string
  command: string
  binding: string
}

export type GnomeShortcutSetup = LegacyShortcutFields & {
  state: 'missing' | 'stale' | 'conflicting' | 'unsupported'
}

export type LegacyShortcutSetup = GnomeShortcutSetup | (LegacyShortcutFields & { state: 'ready' })

export interface LastRun {
  engine: string
  binary: string | null
  modelPath: string | null
  multilingual: boolean | null
  vad: boolean | null
  inferMs: number
  language: string | null
  languageProbability: number | null
  performance?: LastRunPerformance | null
}

export interface LastRunPerformance {
  mode: 'coldCli' | 'coldFallback'
  runtimeSource: 'managed' | 'system' | 'unknown'
  backend: 'cpu' | 'cuda' | 'vulkan' | 'openVino' | 'rocm' | 'unknown'
  device: string | null
  totalMs: number
  audioEncodeMs: number
  childWallMs: number
  parseMs: number
  attemptCount: number
  tuning: {
    threads: number | null
    beamSize: number | null
    bestOf: number | null
    noFallback: boolean | null
  }
}

export interface WhisperModelInfo {
  name: string
  path: string
  family: string
  multilingual: boolean
  quantisation: string | null
  sizeBytes: number
}

export interface EngineAvailability {
  id: string
  available: boolean
  reason: string | null
}

export interface ModelInventory {
  whisper: WhisperModelInfo[]
  vad: string[]
  parakeet: string | null
  engines: EngineAvailability[]
}

export interface LanguageOption {
  code: string
  englishName: string
  group: string
}

export interface LanguageOptions {
  mode: 'multilingual' | 'english' | 'parakeet'
  model: string | null
  options: LanguageOption[]
}

export type ComponentId =
  | 'whisper-runtime'
  | 'whisper-base-q5-1'
  | 'whisper-small'
  | 'whisper-large-v3-turbo-q5-0'
  | 'silero-vad'
  | 'sherpa-runtime'
  | 'parakeet-tdt-06b-v3-int8'

export type SetupPlanId =
  | 'recommended'
  | 'parakeet'
  | 'whisper-base'
  | 'whisper-small'
  | 'whisper-large-v3-turbo'

export type ManagedComponentState =
  | { kind: 'absent'; resumableBytes: number }
  | { kind: 'ready'; version: string; bytes: number; root: string }
  | { kind: 'needs-repair'; reason: string; resumableBytes: number }
  | { kind: 'unsupported'; reason: string }

export interface InstallProgress {
  operationId: string
  component: ComponentId
  phase: 'checking-disk' | 'downloading' | 'verifying' | 'extracting' | 'activating'
  receivedBytes: number
  totalBytes: number
  resumedFromBytes: number
}

export interface ComponentStatus {
  id: ComponentId
  label: string
  managed: ManagedComponentState
  external: Array<{ origin: 'system' | 'external'; path: string }>
  activeOrigin: 'managed' | 'system' | 'external' | null
  activity: InstallProgress | null
}

export interface SetupPlan {
  id: SetupPlanId
  label: string
  components: ComponentId[]
  satisfied: boolean
  downloadBytes: number
  requiredFreeBytes: number
  availableBytes: number | null
  diskReady: boolean
  diskReason: string | null
}

export interface Readiness {
  managedSupported: boolean
  unsupportedReason: string | null
  totalMemoryBytes: number | null
  recommendedModel: ComponentId
  components: ComponentStatus[]
  plans: SetupPlan[]
  microphoneReady: boolean
  speechReady: boolean
  hasSuccessfulDictation: boolean
  firstRunComplete: boolean
  activeOperation: string | null
  activeCancellable: boolean
}

export type SetupEvent =
  | { kind: 'progress'; progress: InstallProgress }
  | { kind: 'finished'; operationId: string }
  | { kind: 'cancelled'; operationId: string }
  | { kind: 'failed'; operationId: string; error: string }

export interface HistoryItem {
  id: string
  text: string
  raw: string
  engine: string
  startedAt: number
  inferMs: number
  injection: string
}

export interface DictionaryItem {
  spoken: string
  written: string
  createdAt: number
}
