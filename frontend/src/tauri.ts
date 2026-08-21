import { invoke } from '@tauri-apps/api/core'
import type { AppStatus, DictionaryItem, HistoryItem } from './types'

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
  modelReady: true,
  engineName: 'Whisper · base.en',
  injectionName: 'ydotool · Wayland',
  shortcut: 'Super+Alt+Space',
}

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

export function getAppStatus(): Promise<AppStatus> {
  return isTauri() ? invoke('get_app_status') : Promise.resolve({ ...previewStatus })
}

export function getHistory(): Promise<HistoryItem[]> {
  return isTauri() ? invoke('get_history') : Promise.resolve([...previewHistory])
}

export function getDictionary(): Promise<DictionaryItem[]> {
  return isTauri() ? invoke('get_dictionary') : Promise.resolve([...previewDictionary])
}

export function addDictionaryEntry(spoken: string, written: string): Promise<DictionaryItem> {
  if (isTauri()) return invoke('add_dictionary_entry', { spoken, written })
  const entry = { spoken, written, createdAt: Math.floor(Date.now() / 1000) }
  previewDictionary = [...previewDictionary, entry]
  return Promise.resolve(entry)
}

export function removeDictionaryEntry(spoken: string, written: string): Promise<boolean> {
  if (isTauri()) return invoke('remove_dictionary_entry', { spoken, written })
  previewDictionary = previewDictionary.filter(
    (entry) => entry.spoken !== spoken || entry.written !== written,
  )
  return Promise.resolve(true)
}

export function toggleRecording(): Promise<void> {
  if (isTauri()) return invoke('toggle_recording')
  if (previewStatus.recording) {
    previewStatus = { ...previewStatus, recording: false, phase: 'Transcribing' }
    window.setTimeout(() => {
      previewStatus = { ...previewStatus, phase: 'Idle' }
    }, 900)
  } else {
    previewStatus = { ...previewStatus, recording: true, phase: 'Recording' }
  }
  return Promise.resolve()
}

export function copyText(text: string): Promise<void> {
  if (isTauri()) return invoke('copy_text', { text })
  return navigator.clipboard.writeText(text)
}
