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
  holdKey: SettingField<string>
  recordSeconds: SettingField<number>
  microphone: SettingField<string>
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
}

export interface LastRun {
  engine: string
  binary: string | null
  modelPath: string | null
  multilingual: boolean | null
  vad: boolean | null
  inferMs: number
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
