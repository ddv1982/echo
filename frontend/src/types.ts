export type View = 'home' | 'history' | 'dictionary' | 'settings'
export type ThemeMode = 'system' | 'light' | 'dark'
export type SettingSource = 'env' | 'file' | 'default'

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
  toggleShortcut: SettingField<string>
  holdKey: SettingField<string>
  recordSeconds: SettingField<number>
  microphone: SettingField<string>
  language: SettingField<string>
}

export interface InputDevice {
  name: string
  isDefault: boolean
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
  shortcut: string
  cleanupName: string
  hudEnabled: boolean
  maxRecordSeconds: number
  settingsPath: string
  version: string
  lastError: string | null
  lastRun: LastRun | null
  languageWarning: string | null
  recordingInProcess: boolean
  currentExe: string
  firstPathHit: string | null
  staleInstalls: string[]
  holdListener: 'native' | 'active' | 'needs-permission' | 'unavailable'
  holdListenerError: string | null
  shortcutBackend: 'portal' | 'x11' | 'unsupported'
  shortcutHealthy: boolean
  shortcutError: string | null
  requestedShortcut: string
  requestedHoldShortcut: string
  effectiveHoldShortcut: string | null
  legacyShortcut: LegacyShortcutSetup | null
  shortcutActivation: string | null
}

export interface LegacyShortcutSetup {
  state: 'missing' | 'stale' | 'conflicting' | 'ready' | 'unsupported'
  detail: string
  command: string
  binding: string
}

export interface LastRun {
  engine: string
  binary: string | null
  modelPath: string | null
  multilingual: boolean | null
  vad: boolean | null
  inferMs: number
  language: string | null
  languageProbability: number | null
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

export interface ModelOffer {
  id: string
  label: string
  filename: string
  url: string
  sizeBytes: number
  runtimeMb: number | null
  multilingual: boolean
  installed: boolean
}

export interface DownloadProgress {
  id: string
  received: number
  total: number
  stage: 'downloading' | 'verifying' | 'done' | 'failed' | 'cancelled'
  error: string | null
}

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
