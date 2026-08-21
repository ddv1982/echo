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
