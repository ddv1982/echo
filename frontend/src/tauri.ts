import { invoke } from '@tauri-apps/api/core'
import type {
  AppStatus,
  DictionaryItem,
  HistoryItem,
  InputDevice,
  LanguageOptions,
  ModelInventory,
  SettingField,
  Settings,
} from './types'

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
  }
}

function richPreviewStatus(): AppStatus {
  return {
    phase: 'Idle',
    lastTranscript: 'This is a test. This is a test.',
    recording: false,
    microphoneReady: true,
    engineName: 'Whisper · small · VAD on',
    engineReady: true,
    injectionName: 'ydotool · Wayland',
    injectionReady: true,
    shortcut: 'Super+Alt+Space',
    cleanupName: 'Rules · fillers and punctuation',
    hudEnabled: true,
    maxRecordSeconds: 60,
    settingsPath: '/tmp/echo-preview/config.json',
    version: '0.1.0',
    lastError: null,
    lastRun: {
      engine: 'whisper-small',
      binary: '/usr/local/bin/whisper-cli',
      modelPath: '/home/user/.cache/echo/ggml-small.bin',
      multilingual: true,
      vad: true,
      inferMs: 1038,
      language: 'de',
      languageProbability: 0.96,
    },
    languageWarning: null,
  }
}

let previewStatus: AppStatus = richPreviewStatus()

let previewSettings: Settings = defaultPreviewSettings()

const previewHistory: HistoryItem[] = [
  {
    id: '1787310400-11',
    text: 'This is a test. This is a test.',
    raw: 'This is a test. This is a test.',
    engine: 'whisper-small',
    startedAt: 1787310400,
    inferMs: 998,
    injection: 'Typed · Ydotool',
  },
  {
    id: '1787310373-9',
    text: 'Open the project settings and update the release notes.',
    raw: 'open the project settings and update the release notes',
    engine: 'whisper-small',
    startedAt: 1787310373,
    inferMs: 1038,
    injection: 'Typed · Ydotool',
  },
  {
    id: '1787310126-8',
    text: 'Claude Code.',
    raw: 'claude code',
    engine: 'whisper-base.en-q5_1',
    startedAt: 1787310126,
    inferMs: 944,
    injection: 'Typed · Ydotool',
  },
]

const previewInventory: ModelInventory = {
  whisper: [
    {
      name: 'base.en-q5_1',
      path: '/home/user/.cache/echo/ggml-base.en-q5_1.bin',
      family: 'base',
      multilingual: false,
      quantisation: 'q5_1',
      sizeBytes: 59_768_832,
    },
    {
      name: 'small',
      path: '/home/user/.cache/echo/ggml-small.bin',
      family: 'small',
      multilingual: true,
      quantisation: null,
      sizeBytes: 488_636_416,
    },
    {
      name: 'large-v3-turbo-q8_0',
      path: '/home/user/.cache/echo/ggml-large-v3-turbo-q8_0.bin',
      family: 'large-v3-turbo',
      multilingual: true,
      quantisation: 'q8_0',
      sizeBytes: 874_610_688,
    },
  ],
  vad: ['/home/user/.cache/echo/ggml-silero-v6.2.0.bin'],
  parakeet: null,
  engines: [
    { id: 'whisper', available: true, reason: null },
    { id: 'parakeet', available: false, reason: 'sherpa-onnx-offline is not on PATH' },
    { id: 'fake', available: true, reason: null },
  ],
}

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

export function listModels(): Promise<ModelInventory> {
  if (isTauri()) return invoke('list_models')
  return Promise.resolve(
    preview
      ? { ...previewInventory }
      : { whisper: [], vad: [], parakeet: null, engines: [] },
  )
}

function defaultPreviewLanguages(): LanguageOptions {
  return {
    mode: 'multilingual',
    model: null,
    options: [
      { code: 'en', englishName: 'english', group: 'common' },
      { code: 'de', englishName: 'german', group: 'common' },
      { code: 'es', englishName: 'spanish', group: 'common' },
      { code: 'fr', englishName: 'french', group: 'common' },
      { code: 'ja', englishName: 'japanese', group: 'all' },
      { code: 'sv', englishName: 'swedish', group: 'all' },
    ],
  }
}

let previewLanguages: LanguageOptions = defaultPreviewLanguages()

export function listLanguages(): Promise<LanguageOptions> {
  if (isTauri()) return invoke('list_languages')
  return Promise.resolve(
    preview ? { ...previewLanguages } : { mode: 'multilingual', model: null, options: [] },
  )
}

export function seedPreviewLanguages(languages: LanguageOptions) {
  previewLanguages = languages
}

export function setSettings(settings: Settings): Promise<Settings> {
  if (isTauri()) return invoke('set_settings', { settings })
  const next = projectPreviewSettings(settings)
  if (preview) {
    previewSettings = next
    applyPreviewStatus(next)
  }
  return Promise.resolve({ ...next })
}

let previewMicTestError: string | null = null

export function seedPreviewSettings(settings: Settings) {
  previewSettings = settings
  applyPreviewStatus(settings)
}

export function seedPreviewMicTestError(message: string) {
  previewMicTestError = message
}

export function seedPreviewStatus(status: Partial<AppStatus>) {
  previewStatus = { ...previewStatus, ...status }
}

export function resetPreviewSettings() {
  previewSettings = defaultPreviewSettings()
  previewStatus = richPreviewStatus()
  previewMicTestError = null
  previewLanguages = defaultPreviewLanguages()
}

const previewDevices: InputDevice[] = [
  { name: 'Built-in Audio Analog Stereo', isDefault: true },
  { name: 'USB Microphone', isDefault: false },
  { name: 'Bluetooth Headset', isDefault: false },
]

export function listInputDevices(): Promise<InputDevice[]> {
  if (isTauri()) return invoke('list_input_devices')
  return Promise.resolve(preview ? previewDevices.map((device) => ({ ...device })) : [])
}

export function testInputDevice(name: string | null): Promise<number> {
  if (isTauri()) return invoke('test_input_device', { name })
  if (previewMicTestError) return Promise.reject(new Error(previewMicTestError))
  return Promise.resolve(name === 'Bluetooth Headset' ? 0 : 0.042)
}

function defaultPreviewSettings(): Settings {
  return projectPreviewSettings({
    engine: { value: null, effective: 'auto', source: 'default' },
    whisperModel: { value: null, effective: '', source: 'default' },
    cleanup: { value: null, effective: 'rules', source: 'default' },
    hud: { value: null, effective: true, source: 'default' },
    holdKey: { value: null, effective: 'RightCtrl', source: 'default' },
    recordSeconds: { value: null, effective: 3, source: 'default' },
    microphone: { value: null, effective: '', source: 'default' },
    language: { value: null, effective: 'en', source: 'default' },
  })
}

function projectPreviewSettings(settings: Settings): Settings {
  const recordValue =
    settings.recordSeconds.value == null
      ? null
      : Math.min(60, Math.max(1, settings.recordSeconds.value))
  return {
    engine: previewField(settings.engine.value, 'auto'),
    whisperModel: previewField(settings.whisperModel.value, ''),
    cleanup: previewField(settings.cleanup.value, 'rules'),
    hud: previewField(settings.hud.value, true),
    holdKey: previewField(settings.holdKey.value, 'RightCtrl'),
    recordSeconds: previewField(recordValue, 3),
    microphone: previewField(settings.microphone.value, ''),
    language: previewField(settings.language.value, 'en'),
  }
}

function previewField<T>(value: T | null, fallback: T): SettingField<T> {
  return {
    value,
    effective: value ?? fallback,
    source: value == null ? 'default' : 'file',
  }
}

function applyPreviewStatus(settings: Settings) {
  const engineNames: Record<string, string> = {
    auto: 'Auto · first installed engine',
    whisper: 'Whisper · small · VAD on',
    parakeet: 'Parakeet · tdt-0.6b-v3',
    fake: 'Fake test engine',
  }
  const cleanupNames: Record<string, string> = {
    off: 'Off',
    rules: 'Rules · fillers and punctuation',
  }
  previewStatus = {
    ...previewStatus,
    engineName: engineNames[settings.engine.effective] ?? settings.engine.effective,
    engineReady: settings.engine.effective !== 'auto',
    cleanupName: cleanupNames[settings.cleanup.effective] ?? settings.cleanup.effective,
    hudEnabled: settings.hud.effective,
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
    version: '0.1.0',
    lastError: null,
    lastRun: null,
    languageWarning: null,
  }
}
