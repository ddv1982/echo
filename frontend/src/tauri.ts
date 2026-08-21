import { invoke } from '@tauri-apps/api/core'
import type { AppStatus, DictionaryItem, HistoryItem, SettingField, Settings } from './types'

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
  }
}

let previewStatus: AppStatus = {
  phase: 'Idle',
  lastTranscript: 'This is a test. This is a test.',
  recording: false,
  microphoneReady: true,
  engineName: 'Whisper · base.en',
  engineReady: true,
  injectionName: 'ydotool · Wayland',
  injectionReady: true,
  shortcut: 'Super+Alt+Space',
  cleanupName: 'Rules · fillers and punctuation',
  hudEnabled: true,
  maxRecordSeconds: 60,
  settingsPath: '/tmp/echo-preview/config.json',
}

let previewSettings: Settings = defaultPreviewSettings()

const previewHistory: HistoryItem[] = [
  {
    id: '1787310400-11',
    text: 'This is a test. This is a test.',
    raw: 'This is a test. This is a test.',
    engine: 'whisper-base.en',
    startedAt: 1787310400,
    inferMs: 998,
    injection: 'Typed · Ydotool',
  },
  {
    id: '1787310373-9',
    text: 'Open the project settings and update the release notes.',
    raw: 'open the project settings and update the release notes',
    engine: 'whisper-base.en',
    startedAt: 1787310373,
    inferMs: 1038,
    injection: 'Typed · Ydotool',
  },
  {
    id: '1787310126-8',
    text: 'Claude Code.',
    raw: 'claude code',
    engine: 'whisper-base.en',
    startedAt: 1787310126,
    inferMs: 944,
    injection: 'Typed · Ydotool',
  },
]

let previewDictionary: DictionaryItem[] = [
  { spoken: 'clawed code', written: 'Claude Code', createdAt: 1787310000 },
  { spoken: 'post grass', written: 'Postgres', createdAt: 1787310100 },
]

function isTauri() {
  return Boolean(window.__TAURI_INTERNALS__)
}

// Preview fixtures back the browser dev server and tests. Vite replaces
// import.meta.env.DEV with false in production builds, so the fixtures and
// these branches are dropped from the shipped bundle.
const preview = import.meta.env.DEV

export function getAppStatus(): Promise<AppStatus> {
  if (isTauri()) return invoke('get_app_status')
  return Promise.resolve(preview ? { ...previewStatus } : initialPreviewStatus())
}

export function getHistory(): Promise<HistoryItem[]> {
  if (isTauri()) return invoke('get_history')
  return Promise.resolve(preview ? [...previewHistory] : [])
}

export function getDictionary(): Promise<DictionaryItem[]> {
  if (isTauri()) return invoke('get_dictionary')
  return Promise.resolve(preview ? [...previewDictionary] : [])
}

export function addDictionaryEntry(spoken: string, written: string): Promise<DictionaryItem> {
  if (isTauri()) return invoke('add_dictionary_entry', { spoken, written })
  const entry = { spoken, written, createdAt: Math.floor(Date.now() / 1000) }
  if (preview) previewDictionary = [...previewDictionary, entry]
  return Promise.resolve(entry)
}

export function removeDictionaryEntry(spoken: string, written: string): Promise<boolean> {
  if (isTauri()) return invoke('remove_dictionary_entry', { spoken, written })
  if (preview) {
    previewDictionary = previewDictionary.filter(
      (entry) => entry.spoken !== spoken || entry.written !== written,
    )
  }
  return Promise.resolve(true)
}

export function toggleRecording(): Promise<void> {
  if (isTauri()) return invoke('toggle_recording')
  if (preview) {
    if (previewStatus.recording) {
      previewStatus = { ...previewStatus, recording: false, phase: 'Transcribing' }
      window.setTimeout(() => {
        previewStatus = { ...previewStatus, phase: 'Idle' }
      }, 900)
    } else {
      previewStatus = { ...previewStatus, recording: true, phase: 'Recording' }
    }
  }
  return Promise.resolve()
}

export function copyText(text: string): Promise<void> {
  if (isTauri()) return invoke('copy_text', { text })
  return navigator.clipboard.writeText(text)
}

export function getSettings(): Promise<Settings> {
  if (isTauri()) return invoke('get_settings')
  return Promise.resolve(preview ? { ...previewSettings } : defaultPreviewSettings())
}

export function setSettings(settings: Settings): Promise<Settings> {
  if (isTauri()) return invoke('set_settings', { settings })
  const next = projectPreviewSettings(settings)
  if (preview) previewSettings = next
  return Promise.resolve({ ...next })
}

function defaultPreviewSettings(): Settings {
  return projectPreviewSettings({
    engine: { value: null, effective: 'auto', source: 'default' },
    whisperModel: { value: null, effective: 'base.en', source: 'default' },
    cleanup: { value: null, effective: 'rules', source: 'default' },
    hud: { value: null, effective: true, source: 'default' },
    holdKey: { value: null, effective: 'RightCtrl', source: 'default' },
    recordSeconds: { value: null, effective: 3, source: 'default' },
  })
}

function projectPreviewSettings(settings: Settings): Settings {
  const recordValue =
    settings.recordSeconds.value == null
      ? null
      : Math.min(60, Math.max(1, settings.recordSeconds.value))
  return {
    engine: previewField(settings.engine.value, 'auto'),
    whisperModel: previewField(settings.whisperModel.value, 'base.en'),
    cleanup: previewField(settings.cleanup.value, 'rules'),
    hud: previewField(settings.hud.value, true),
    holdKey: previewField(settings.holdKey.value, 'RightCtrl'),
    recordSeconds: previewField(recordValue, 3),
  }
}

function previewField<T>(value: T | null, fallback: T): SettingField<T> {
  return {
    value,
    effective: value ?? fallback,
    source: value == null ? 'default' : 'file',
  }
}

function initialPreviewStatus(): AppStatus {
  return {
    phase: 'Idle',
    lastTranscript: null,
    recording: false,
    microphoneReady: false,
    engineName: 'Not connected',
    engineReady: false,
    injectionName: 'Not connected',
    injectionReady: false,
    shortcut: 'Super+Alt+Space',
    cleanupName: 'Rules · fillers and punctuation',
    hudEnabled: true,
    maxRecordSeconds: 60,
    settingsPath: '/tmp/echo-preview/config.json',
  }
}
