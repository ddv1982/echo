import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AppStatus,
  DictionaryItem,
  DownloadProgress,
  HistoryItem,
  InputDevice,
  LanguageOptions,
  LegacyShortcutSetup,
  ModelInventory,
  ModelOffer,
  ShortcutStatus,
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
    shortcut: {
      kind: 'active',
      desired: 'Super+Alt+Space',
      effective: 'Super+Alt+Space',
      backend: 'portal',
      activation: null,
      verificationIdentity: 'portal:Super+Alt+Space',
    },
    cleanupName: 'Rules · fillers and punctuation',
    hudEnabled: true,
    maxRecordSeconds: 60,
    settingsPath: '/tmp/echo-preview/config.json',
    version: __APP_VERSION__,
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
    recordingInProcess: false,
    currentExe: '/usr/bin/echo-desktop',
    firstPathHit: '/usr/bin/echo-desktop',
    staleInstalls: [],
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

let previewInventory: ModelInventory = defaultPreviewInventory()

function defaultPreviewInventory(): ModelInventory {
  return {
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
  vad: [],
  parakeet: null,
  engines: [
    { id: 'whisper', available: true, reason: null },
    { id: 'parakeet', available: false, reason: 'sherpa-onnx-offline is not on PATH' },
  ],
  }
}

export function seedPreviewInventory(inventory: ModelInventory) {
  previewInventory = inventory
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

export function getShortcutStatus(): Promise<ShortcutStatus> {
  if (isTauri()) return invoke('get_shortcut_status')
  return Promise.resolve(previewStatus.shortcut)
}

export function retryShortcut(): Promise<ShortcutStatus> {
  if (isTauri()) return invoke('retry_shortcut')
  return Promise.resolve(previewStatus.shortcut)
}

export function repairLegacyShortcut(): Promise<LegacyShortcutSetup> {
  if (isTauri()) return invoke('repair_legacy_shortcut')
  const shortcut = previewStatus.shortcut
  if (shortcut.kind !== 'gnome-setup') {
    return Promise.reject(new Error('This session does not need a legacy shortcut.'))
  }
  const setup = shortcut.setup
  if (setup.state === 'conflicting' || setup.state === 'unsupported') {
    return Promise.reject(new Error(setup.detail))
  }
  const repaired: LegacyShortcutSetup = {
    ...setup,
    state: 'ready',
    detail: 'GNOME owns this Echo shortcut and its command is current.',
  }
  previewStatus = {
    ...previewStatus,
    shortcut: {
      kind: 'gnome-ready',
      desired: shortcut.desired,
      effective: shortcut.desired,
      detail: repaired.detail,
      command: repaired.command,
      binding: repaired.binding,
      activation: null,
      verificationIdentity: `gnome:${repaired.binding}:${repaired.command}`,
    },
  }
  return Promise.resolve(repaired)
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
      previewStatus = { ...previewStatus, recording: false, recordingInProcess: false, phase: 'Transcribing' }
      window.setTimeout(() => {
        previewStatus = { ...previewStatus, phase: 'Idle' }
      }, 900)
    } else {
      previewStatus = {
        ...previewStatus,
        recording: true,
        recordingInProcess: true,
        phase: 'Recording',
      }
    }
  }
  return Promise.resolve()
}

export function getRecordingLevel(): Promise<number> {
  if (isTauri()) return invoke('get_recording_level')
  if (preview && previewStatus.recording && previewStatus.recordingInProcess) {
    // A plausible speech-like level for the dev server and tests.
    const t = Date.now() / 1000
    const level = Math.max(0, 0.06 + 0.22 * Math.abs(Math.sin(t * 2.1) * Math.sin(t * 0.7)))
    return Promise.resolve(level)
  }
  return Promise.resolve(0)
}

export function copyText(text: string): Promise<void> {
  if (isTauri()) return invoke('copy_text', { text })
  return navigator.clipboard.writeText(text)
}

let previewRemoveStaleError: string | null = null

export function seedPreviewRemoveStaleError(message: string) {
  previewRemoveStaleError = message
}

export function removeStaleInstalls(): Promise<string[]> {
  if (isTauri()) return invoke('remove_stale_installs')
  if (previewRemoveStaleError) return Promise.reject(new Error(previewRemoveStaleError))
  const removed = previewStatus.staleInstalls
  if (preview) {
    previewStatus = {
      ...previewStatus,
      staleInstalls: [],
      firstPathHit: previewStatus.currentExe || previewStatus.firstPathHit,
    }
  }
  return Promise.resolve(removed)
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

function defaultPreviewOffers(): ModelOffer[] {
  return [
    {
      id: 'base-en-q5_1',
      label: 'Fast, English',
      filename: 'ggml-base.en-q5_1.bin',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q5_1.bin',
      sizeBytes: 59_721_011,
      runtimeMb: 388,
      multilingual: false,
      installed: true,
    },
    {
      id: 'small',
      label: 'Balanced, multilingual',
      filename: 'ggml-small.bin',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin',
      sizeBytes: 487_601_967,
      runtimeMb: 852,
      multilingual: true,
      installed: false,
    },
    {
      id: 'large-v3-turbo-q5_0',
      label: 'Best, multilingual',
      filename: 'ggml-large-v3-turbo-q5_0.bin',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin',
      sizeBytes: 574_041_195,
      runtimeMb: null,
      multilingual: true,
      installed: false,
    },
    {
      id: 'silero-vad',
      label: 'Silence detection',
      filename: 'ggml-silero-v6.2.0.bin',
      url: 'https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin',
      sizeBytes: 885_098,
      runtimeMb: null,
      multilingual: false,
      installed: false,
    },
  ]
}

let previewOffers: ModelOffer[] = defaultPreviewOffers()

const previewDownloadListeners = new Set<(progress: DownloadProgress) => void>()
const previewDownloadTimers = new Map<string, Array<ReturnType<typeof setTimeout>>>()

export function listModelOffers(): Promise<ModelOffer[]> {
  if (isTauri()) return invoke('list_model_offers')
  return Promise.resolve(preview ? previewOffers.map((offer) => ({ ...offer })) : [])
}

export function downloadModel(id: string): Promise<void> {
  if (isTauri()) return invoke('download_model', { id })
  if (!preview) return Promise.resolve()
  const offer = previewOffers.find((candidate) => candidate.id === id)
  if (!offer) return Promise.reject(new Error(`unknown model offer ${id}`))
  const emit = (stage: DownloadProgress['stage'], received: number, error: string | null = null) => {
    const progress: DownloadProgress = { id, received, total: offer.sizeBytes, stage, error }
    previewDownloadListeners.forEach((listener) => listener(progress))
  }
  emit('downloading', 0)
  const midway = setTimeout(() => emit('downloading', Math.floor(offer.sizeBytes / 2)), 300)
  const verifying = setTimeout(() => emit('verifying', offer.sizeBytes), 700)
  const done = setTimeout(() => {
    previewOffers = previewOffers.map((candidate) =>
      candidate.id === id ? { ...candidate, installed: true } : candidate,
    )
    emit('done', offer.sizeBytes)
  }, 1100)
  previewDownloadTimers.set(id, [midway, verifying, done])
  return Promise.resolve()
}

export function cancelDownload(id: string): Promise<boolean> {
  if (isTauri()) return invoke('cancel_download', { id })
  if (preview) {
    const timers = previewDownloadTimers.get(id)
    if (timers) {
      timers.forEach(clearTimeout)
      previewDownloadTimers.delete(id)
      previewDownloadListeners.forEach((listener) =>
        listener({ id, received: 0, total: 0, stage: 'cancelled', error: null }),
      )
      return Promise.resolve(true)
    }
  }
  return Promise.resolve(false)
}

export function onDownloadProgress(
  handler: (progress: DownloadProgress) => void,
): Promise<() => void> {
  if (isTauri()) {
    return listen<DownloadProgress>('model-download-progress', (event) => handler(event.payload))
  }
  previewDownloadListeners.add(handler)
  return Promise.resolve(() => previewDownloadListeners.delete(handler))
}

export function seedPreviewOffers(offers: ModelOffer[]) {
  previewOffers = offers
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
  previewRemoveStaleError = null
  previewLanguages = defaultPreviewLanguages()
  previewInventory = defaultPreviewInventory()
  previewOffers = defaultPreviewOffers()
  previewDownloadTimers.forEach((timers) => timers.forEach(clearTimeout))
  previewDownloadTimers.clear()
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
    recordSeconds: { value: null, effective: 3, source: 'default' },
    microphone: { value: null, effective: '', source: 'default' },
    language: { value: null, effective: 'auto', source: 'default' },
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
    recordSeconds: previewField(recordValue, 3),
    microphone: previewField(settings.microphone.value, ''),
    language: previewField(settings.language.value, 'auto'),
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
    shortcut: {
      kind: 'unsupported',
      desired: 'Super+Alt+Space',
      detail: 'Not connected',
    },
    cleanupName: 'Rules · fillers and punctuation',
    hudEnabled: true,
    maxRecordSeconds: 60,
    settingsPath: '/tmp/echo-preview/config.json',
    version: __APP_VERSION__,
    lastError: null,
    lastRun: null,
    languageWarning: null,
    recordingInProcess: false,
    currentExe: '',
    firstPathHit: null,
    staleInstalls: [],
  }
}
