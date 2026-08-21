export type View = 'home' | 'history' | 'dictionary' | 'settings'
export type ThemeMode = 'system' | 'light' | 'dark'

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
