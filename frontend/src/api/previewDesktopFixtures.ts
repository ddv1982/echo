import type {
  AppStatus,
  ComponentId,
  DictionaryItem,
  GpuDevice,
  HistoryItem,
  InputDevice,
  LanguageOptions,
  MicrophoneSnapshot,
  ModelInventory,
  Readiness,
  RecordingPolicy,
} from '../generated/ipc'

export const PREVIEW_RECORDING_POLICY: RecordingPolicy = {
  minimumSeconds: 1,
  defaultSeconds: 600,
  maximumSeconds: 600,
  presetsSeconds: [30, 60, 120, 300, 600],
}

export const PREVIEW_TRAINING_TRANSCRIPTS: readonly [string, ...string[]] = [
  'kuber netties',
  'cooper net ease',
  'Kubernetes',
  'kuber netties',
  'cube er netties',
]

export function richPreviewStatus(): AppStatus {
  return {
    phase: 'Idle',
    lastTranscript: 'This is a test. This is a test.',
    lastHistoryId: null,
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
    hudEnabled: true,
    recordingLimitSeconds: PREVIEW_RECORDING_POLICY.defaultSeconds,
    recordingPolicy: structuredClone(PREVIEW_RECORDING_POLICY),
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
      performance: {
        mode: 'coldCli',
        runtimeSource: 'system',
        backend: 'vulkan',
        device: 'Intel Iris Xe',
        totalMs: 1038,
        audioEncodeMs: 3,
        childWallMs: 1032,
        parseMs: 1,
        attemptCount: 1,
        tuning: { threads: 4, beamSize: 5, bestOf: 5, noFallback: false },
        accelerationSkip: null,
        recovery: null,
      },
    },
    languageWarning: null,
    recordingInProcess: false,
    currentExe: '/usr/bin/echo-desktop',
    firstPathHit: '/usr/bin/echo-desktop',
    staleInstalls: [],
  }
}

export function defaultPreviewHistory(): HistoryItem[] {
  return [
    { id: '1787310400-11', text: 'This is a test. This is a test.', raw: 'This is a test. This is a test.', engine: 'whisper-small', startedAt: 1787310400, inferMs: 998, injection: 'Typed · Ydotool' },
    { id: '1787310373-9', text: 'Open the project settings and update the release notes.', raw: 'open the project settings and update the release notes', engine: 'whisper-small', startedAt: 1787310373, inferMs: 1038, injection: 'Typed · Ydotool' },
    { id: '1787310126-8', text: 'Claude Code.', raw: 'claude code', engine: 'whisper-base.en-q5_1', startedAt: 1787310126, inferMs: 944, injection: 'Typed · Ydotool' },
  ]
}

export function defaultPreviewInventory(): ModelInventory {
  return {
    whisper: [
      { name: 'base.en-q5_1', path: '/home/user/.cache/echo/ggml-base.en-q5_1.bin', family: 'base', multilingual: false, quantisation: 'q5_1', sizeBytes: 59_768_832 },
      { name: 'small', path: '/home/user/.cache/echo/ggml-small.bin', family: 'small', multilingual: true, quantisation: null, sizeBytes: 488_636_416 },
      { name: 'large-v3-turbo-q8_0', path: '/home/user/.cache/echo/ggml-large-v3-turbo-q8_0.bin', family: 'large-v3-turbo', multilingual: true, quantisation: 'q8_0', sizeBytes: 874_610_688 },
    ],
    vad: [],
    parakeet: null,
    engines: [
      { id: 'whisper', available: true, reason: null },
      { id: 'parakeet', available: false, reason: 'sherpa-onnx-offline is not on PATH' },
    ],
  }
}

export function defaultPreviewDictionary(): DictionaryItem[] {
  return [
    { spoken: 'clawed code', written: 'Claude Code', createdAt: 1787310000 },
    { spoken: 'post grass', written: 'Postgres', createdAt: 1787310100 },
  ]
}

export function defaultPreviewLanguages(): LanguageOptions {
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

export function defaultPreviewGpuDevices(): GpuDevice[] {
  return [
    { id: { deviceUUID: '8680a6460c0000000002000000000000', driverUUID: 'ee99561e45e1e718c6121d36d8345582' }, name: 'Intel(R) Iris(R) Xe Graphics (ADL GT2)', vendorId: 0x8086, deviceId: 0x46a6, drmDriver: 'i915', software: false },
    { id: { deviceUUID: '1002744c0000000000010000000000aa', driverUUID: '3f7b1c9a45e1e718c6121d36d8340000' }, name: 'AMD Radeon RX 7800 XT (RADV)', vendorId: 0x1002, deviceId: 0x747e, drmDriver: 'amdgpu', software: false },
    { id: { deviceUUID: '00050100000000000000000000000000', driverUUID: '00050100000000000000000000000001' }, name: 'llvmpipe (LLVM 20.1.8)', vendorId: 0x10005, deviceId: 0x0, drmDriver: null, software: true },
  ]
}

export function defaultPreviewDevices(): InputDevice[] {
  const advancedDeviceOptions: Array<readonly [id: string, label: string]> = [
    ['alsa:pipewire', 'PipeWire Sound Server'],
    ['alsa:pulse', 'PulseAudio Sound Server'],
    ['alsa:downmix', 'Plugin for channel downmix'],
    ['alsa:upmix', 'Plugin for channel upmix'],
    ['alsa:speex', 'Plugin using Speex DSP'],
    ['alsa:speexrate', 'Rate Converter Using Speex'],
    ['alsa:dsnoop:CARD=sofhdadsp,DEV=6', 'sof-hda-dsp,'],
    ['alsa:dsnoop:CARD=sofhdadsp,DEV=7', 'sof-hda-dsp,'],
  ]
  const advancedDevices: InputDevice[] = advancedDeviceOptions.map(([id, label]) => ({
    id, label, isDefault: false, manufacturer: null, deviceType: 'Virtual', interfaceType: 'Virtual', address: null, driver: 'ALSA', extended: [], host: 'alsa', transport: 'virtual', tier: 'advanced', hint: 'Virtual endpoint',
  }))
  return [
    { id: 'pipewire:alsa_input.pci-0000_00_1f.3.analog-stereo', label: 'Built-in Audio', isDefault: false, manufacturer: 'Intel', deviceType: 'Microphone', interfaceType: 'Built-in', address: null, driver: 'PipeWire', extended: [], host: 'pipe-wire', transport: 'built-in', tier: 'primary', hint: 'Built in · Microphone · Intel' },
    { id: 'pipewire:bluez_input.48_5F_99_00_11_22.0', label: 'Jabra Elite 8 Active', isDefault: false, manufacturer: 'Jabra', deviceType: 'Headset', interfaceType: 'Bluetooth', address: '48:5F:99:00:11:22', driver: 'PipeWire', extended: [], host: 'pipe-wire', transport: 'bluetooth', tier: 'primary', hint: 'Bluetooth · Headset · Jabra' },
    { id: 'pipewire:alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo', label: 'USB Microphone', isDefault: false, manufacturer: 'Focusrite', deviceType: 'Microphone', interfaceType: 'USB', address: '1-2', driver: 'PipeWire', extended: [], host: 'pipe-wire', transport: 'usb', tier: 'primary', hint: 'USB · Microphone · Focusrite' },
    { id: 'pipewire:alsa_input.usb-Logitech_USB_Headset-00.mono-fallback', label: 'USB Microphone', isDefault: false, manufacturer: 'Logitech', deviceType: 'Headset', interfaceType: 'USB', address: '1-3', driver: 'PipeWire', extended: [], host: 'pipe-wire', transport: 'usb', tier: 'primary', hint: 'USB · Headset · Logitech' },
    ...advancedDevices,
  ]
}

export function defaultPreviewSystemDefault(): InputDevice {
  return { id: 'pipewire:input_default', label: 'System default', isDefault: true, manufacturer: null, deviceType: 'Microphone', interfaceType: 'Virtual', address: null, driver: 'PipeWire', extended: [], host: 'pipe-wire', transport: 'virtual', tier: 'advanced', hint: 'Follows the Linux system default' }
}

export function defaultPreviewMicrophones(devices: InputDevice[]): MicrophoneSnapshot {
  const systemDefault = defaultPreviewSystemDefault()
  return { host: 'pipe-wire', source: 'default', systemDefault, systemDefaultIsProxy: true, devices, selection: { kind: 'system-default', active: systemDefault }, enumerationWarning: null }
}

export function defaultPreviewReadiness(): Readiness {
  const sources: Array<{ id: ComponentId; label: string; path: string; origin: 'system' | 'external' }> = [
    { id: 'whisper-runtime', label: 'Whisper runtime', path: '/usr/bin/whisper-cli', origin: 'system' },
    { id: 'whisper-base-q51', label: 'Base multilingual Q5_1', path: '', origin: 'external' },
    { id: 'whisper-small', label: 'Small multilingual', path: '/home/user/.cache/echo/ggml-small.bin', origin: 'external' },
    { id: 'whisper-large-v3-turbo-q50', label: 'Large v3 Turbo Q5_0', path: '', origin: 'external' },
    { id: 'silero-vad', label: 'Silero voice detection', path: '/home/user/.cache/echo/ggml-silero-v6.2.0.bin', origin: 'external' },
    { id: 'whisper-vulkan-runtime', label: 'Whisper GPU runtime', path: '', origin: 'system' },
    { id: 'sherpa-runtime', label: 'sherpa-onnx runtime', path: '', origin: 'system' },
    { id: 'parakeet-tdt06b-v3-int8', label: 'Parakeet TDT 0.6b v3', path: '', origin: 'external' },
  ]
  return {
    managedSupported: true,
    unsupportedReason: null,
    totalMemoryBytes: 8 * 1024 * 1024 * 1024,
    recommendedModel: 'whisper-small',
    components: sources.map(({ id, label, path, origin }) => ({ id, label, managed: { kind: 'absent', resumableBytes: 0 }, external: path ? [{ origin, path }] : [], activeOrigin: path ? origin : null, activity: null })),
    plans: [
      { id: 'recommended', label: 'Recommended', components: ['whisper-runtime', 'whisper-small', 'silero-vad'], satisfied: true, downloadBytes: 0, requiredFreeBytes: 0, availableBytes: 10_000_000_000, diskReady: true, diskReason: null },
      { id: 'parakeet', label: 'Parakeet', components: ['sherpa-runtime', 'parakeet-tdt06b-v3-int8'], satisfied: false, downloadBytes: 848_526_547, requiredFreeBytes: 1_500_000_000, availableBytes: 10_000_000_000, diskReady: true, diskReason: null },
      { id: 'whisper-base', label: 'Whisper base', components: ['whisper-runtime', 'whisper-base-q51'], satisfied: false, downloadBytes: 141_000_000, requiredFreeBytes: 300_000_000, availableBytes: 10_000_000_000, diskReady: true, diskReason: null },
      { id: 'whisper-small', label: 'Whisper small', components: ['whisper-runtime', 'whisper-small', 'silero-vad'], satisfied: true, downloadBytes: 0, requiredFreeBytes: 0, availableBytes: 10_000_000_000, diskReady: true, diskReason: null },
      { id: 'whisper-large-v3-turbo', label: 'Whisper Large v3 Turbo Q5_0', components: ['whisper-runtime', 'whisper-large-v3-turbo-q50', 'silero-vad'], satisfied: false, downloadBytes: 574_041_195, requiredFreeBytes: 1_200_000_000, availableBytes: 10_000_000_000, diskReady: true, diskReason: null },
    ],
    microphoneReady: true,
    speechReady: true,
    hasSuccessfulDictation: true,
    firstRunComplete: true,
    activeOperation: null,
    activeCancellable: false,
  }
}
