import type {
  AppStatus,
  DictionaryItem,
  GpuDevice,
  HistoryItem,
  InputDevice,
  LanguageOptions,
  LegacyShortcutSetup,
  ModelInventory,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  Readiness,
  RecordingPolicy,
  SetupEvent,
  SetupPlanId,
  ComponentId,
  ShortcutStatus,
  SettingField,
  Settings,
} from '../generated/ipc'
import type { DesktopApi } from './DesktopApi'

export interface PreviewDesktopApi extends DesktopApi {
  richPreviewStatus(): AppStatus
  seedPreviewInventory(inventory: ModelInventory): void
  seedPreviewRemoveStaleError(message: string): void
  seedPreviewLanguages(languages: LanguageOptions): void
  seedPreviewLanguagesError(message: string): void
  seedPreviewSettings(settings: Settings): void
  seedPreviewMicTestError(message: string): void
  seedPreviewGpuDevices(devices: GpuDevice[]): void
  seedPreviewStatus(status: Partial<AppStatus>): void
  resetPreviewSettings(): void
  seedPreviewMicrophones(snapshot: MicrophoneSnapshot): void
  seedPreviewReadiness(readiness: Readiness): void
}

export function createPreviewDesktopApi(): PreviewDesktopApi {

  const PREVIEW_RECORDING_POLICY: RecordingPolicy = {
    minimumSeconds: 1,
    defaultSeconds: 600,
    maximumSeconds: 600,
    presetsSeconds: [30, 60, 120, 300, 600],
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
      recordingLimitSeconds: PREVIEW_RECORDING_POLICY.defaultSeconds,
      recordingPolicy: PREVIEW_RECORDING_POLICY,
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
          tuning: {
            threads: 4,
            beamSize: 5,
            bestOf: 5,
            noFallback: false,
          },
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

  let previewStatus: AppStatus = richPreviewStatus()

  let previewSettings: Settings = defaultPreviewSettings()
  let previewRecordingDeadline: number | null = null
  const previewTimers = new Set<number>()

  function schedulePreview(callback: () => void, delayMs: number): number {
    const timer = window.setTimeout(() => {
      previewTimers.delete(timer)
      callback()
    }, delayMs)
    previewTimers.add(timer)
    return timer
  }

  function clearPreviewTimer(timer: number): void {
    window.clearTimeout(timer)
    previewTimers.delete(timer)
  }

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

  function seedPreviewInventory(inventory: ModelInventory) {
    previewInventory = inventory
  }

  let previewDictionary: DictionaryItem[] = defaultPreviewDictionary()

  function defaultPreviewDictionary(): DictionaryItem[] {
    return [
      { spoken: 'clawed code', written: 'Claude Code', createdAt: 1787310000 },
      { spoken: 'post grass', written: 'Postgres', createdAt: 1787310100 },
    ]
  }

  function getAppStatus(): Promise<AppStatus> {
    return Promise.resolve({ ...previewStatus })
  }

  function getShortcutStatus(): Promise<ShortcutStatus> {
    return Promise.resolve(previewStatus.shortcut)
  }

  function retryShortcut(): Promise<ShortcutStatus> {
    return Promise.resolve(previewStatus.shortcut)
  }

  function repairLegacyShortcut(): Promise<LegacyShortcutSetup> {
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

  function getHistory(): Promise<HistoryItem[]> {
    return Promise.resolve([...previewHistory])
  }

  function getDictionary(): Promise<DictionaryItem[]> {
    return Promise.resolve([...previewDictionary])
  }

  function addDictionaryEntry(spoken: string, written: string): Promise<DictionaryItem> {
    const entry = { spoken, written, createdAt: Math.floor(Date.now() / 1000) }
    previewDictionary = [...previewDictionary, entry]
    return Promise.resolve(entry)
  }

  function removeDictionaryEntry(spoken: string, written: string): Promise<boolean> {
    previewDictionary = previewDictionary.filter(
      (entry) => entry.spoken !== spoken || entry.written !== written,
    )
    return Promise.resolve(true)
  }

  function toggleRecording(): Promise<void> {
    if (previewStatus.recording) {
      stopPreviewRecording()
    } else {
      const limit = previewSettings.recordSeconds.effective
      previewStatus = {
        ...previewStatus,
        recording: true,
        recordingInProcess: true,
        phase: 'Recording',
        recordingLimitSeconds: limit,
      }
      previewRecordingDeadline = schedulePreview(() => {
        previewRecordingDeadline = null
        stopPreviewRecording()
      }, limit * 1000)
    }
    return Promise.resolve()
  }

  function stopRecording(activation: string): Promise<boolean> {
    const currentActivation =
      'activation' in previewStatus.shortcut ? previewStatus.shortcut.activation : null
    if (currentActivation !== activation) return Promise.resolve(false)
    return Promise.resolve(stopPreviewRecording())
  }

  function stopPreviewRecording() {
    if (previewRecordingDeadline != null) {
      clearPreviewTimer(previewRecordingDeadline)
      previewRecordingDeadline = null
    }
    if (!previewStatus.recording) return false
    previewStatus = {
      ...previewStatus,
      recording: false,
      recordingInProcess: false,
      phase: 'Transcribing',
    }
    schedulePreview(() => {
      previewStatus = { ...previewStatus, phase: 'Idle' }
    }, 900)
    return true
  }

  function getRecordingLevel(): Promise<number> {
    if (previewStatus.recording && previewStatus.recordingInProcess) {
      const t = Date.now() / 1000
      const level = Math.max(0, 0.06 + 0.22 * Math.abs(Math.sin(t * 2.1) * Math.sin(t * 0.7)))
      return Promise.resolve(level)
    }
    return Promise.resolve(0)
  }

  function copyText(text: string): Promise<void> {
    return navigator.clipboard.writeText(text)
  }

  let previewRemoveStaleError: string | null = null

  function seedPreviewRemoveStaleError(message: string) {
    previewRemoveStaleError = message
  }

  function removeStaleInstalls(): Promise<string[]> {
    if (previewRemoveStaleError) return Promise.reject(new Error(previewRemoveStaleError))
    const removed = previewStatus.staleInstalls
    previewStatus = {
      ...previewStatus,
      staleInstalls: [],
      firstPathHit: previewStatus.currentExe || previewStatus.firstPathHit,
    }
    return Promise.resolve(removed)
  }

  function getSettings(): Promise<Settings> {
    return Promise.resolve({ ...previewSettings })
  }

  function listModels(): Promise<ModelInventory> {
    return Promise.resolve({ ...previewInventory })
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
  let previewLanguagesError: string | null = null

  function listLanguages(): Promise<LanguageOptions> {
    if (previewLanguagesError) return Promise.reject(new Error(previewLanguagesError))
    return Promise.resolve({ ...previewLanguages })
  }

  function seedPreviewLanguages(languages: LanguageOptions) {
    previewLanguages = languages
  }

  function seedPreviewLanguagesError(message: string) {
    previewLanguagesError = message
  }


  function setSettings(settings: Settings): Promise<Settings> {
    const next = projectPreviewSettings(settings)
    previewSettings = next
    if (next.engine.effective === 'parakeet') {
      previewLanguages = {
        mode: 'parakeet',
        model: 'parakeet-tdt-0.6b-v3-int8',
        options: Array.from({ length: 25 }, (_, index) => ({
          code: `p${index}`,
          englishName: `parakeet language ${index}`,
          group: 'all',
        })),
      }
    } else if (previewLanguages.mode === 'parakeet') {
      previewLanguages = defaultPreviewLanguages()
    }
    applyPreviewStatus(next)
    return Promise.resolve({ ...next })
  }

  let previewMicTestError: string | null = null

  function seedPreviewSettings(settings: Settings) {
    previewSettings = settings
    applyPreviewStatus(settings)
  }

  function seedPreviewMicTestError(message: string) {
    previewMicTestError = message
  }

  let previewGpuDevices: GpuDevice[] = defaultPreviewGpuDevices()

  function defaultPreviewGpuDevices(): GpuDevice[] {
    return [
      {
        id: {
          deviceUUID: '8680a6460c0000000002000000000000',
          driverUUID: 'ee99561e45e1e718c6121d36d8345582',
        },
        name: 'Intel(R) Iris(R) Xe Graphics (ADL GT2)',
        vendorId: 0x8086,
        deviceId: 0x46a6,
        drmDriver: 'i915',
        software: false,
      },
      {
        id: {
          deviceUUID: '1002744c0000000000010000000000aa',
          driverUUID: '3f7b1c9a45e1e718c6121d36d8340000',
        },
        name: 'AMD Radeon RX 7800 XT (RADV)',
        vendorId: 0x1002,
        deviceId: 0x747e,
        drmDriver: 'amdgpu',
        software: false,
      },
      {
        id: {
          deviceUUID: '00050100000000000000000000000000',
          driverUUID: '00050100000000000000000000000001',
        },
        name: 'llvmpipe (LLVM 20.1.8)',
        vendorId: 0x10005,
        deviceId: 0x0,
        drmDriver: null,
        software: true,
      },
    ]
  }

  function listGpuDevices(refresh = false): Promise<GpuDevice[]> {
    void refresh
    return Promise.resolve(previewGpuDevices.map((device) => ({ ...device })))
  }

  function seedPreviewGpuDevices(devices: GpuDevice[]) {
    previewGpuDevices = devices
  }

  function seedPreviewStatus(status: Partial<AppStatus>) {
    previewStatus = { ...previewStatus, ...status }
  }

  function resetPreviewSettings() {
    previewTimers.forEach((timer) => window.clearTimeout(timer))
    previewTimers.clear()
    previewRecordingDeadline = null
    previewSettings = defaultPreviewSettings()
    previewStatus = richPreviewStatus()
    previewDictionary = defaultPreviewDictionary()
    previewMicTestError = null
    previewRemoveStaleError = null
    previewLanguages = defaultPreviewLanguages()
    previewLanguagesError = null
    previewInventory = defaultPreviewInventory()
    previewDevices = defaultPreviewDevices()
    previewMicrophones = defaultPreviewMicrophones(previewDevices)
    previewReadiness = defaultPreviewReadiness()
    previewGpuDevices = defaultPreviewGpuDevices()
    previewSetupListeners.clear()
    previewSetupTimers.clear()
  }

  function defaultPreviewDevices(): InputDevice[] {
    const advancedDevices: InputDevice[] = [
      ['alsa:pipewire', 'PipeWire Sound Server'],
      ['alsa:pulse', 'PulseAudio Sound Server'],
      ['alsa:downmix', 'Plugin for channel downmix'],
      ['alsa:upmix', 'Plugin for channel upmix'],
      ['alsa:speex', 'Plugin using Speex DSP'],
      ['alsa:speexrate', 'Rate Converter Using Speex'],
      ['alsa:dsnoop:CARD=sofhdadsp,DEV=6', 'sof-hda-dsp,'],
      ['alsa:dsnoop:CARD=sofhdadsp,DEV=7', 'sof-hda-dsp,'],
    ].map(([id, label]) => ({
      id,
      label,
      isDefault: false,
      manufacturer: null,
      deviceType: 'Virtual',
      interfaceType: 'Virtual',
      address: null,
      driver: 'ALSA',
      extended: [],
      host: 'alsa',
      transport: 'virtual',
      tier: 'advanced',
      hint: 'Virtual endpoint',
    }))
    return [
      {
        id: 'pipewire:alsa_input.pci-0000_00_1f.3.analog-stereo',
        label: 'Built-in Audio',
        isDefault: false,
        manufacturer: 'Intel',
        deviceType: 'Microphone',
        interfaceType: 'Built-in',
        address: null,
        driver: 'PipeWire',
        extended: [],
        host: 'pipe-wire',
        transport: 'built-in',
        tier: 'primary',
        hint: 'Built in · Microphone · Intel',
      },
      {
        id: 'pipewire:bluez_input.48_5F_99_00_11_22.0',
        label: 'Jabra Elite 8 Active',
        isDefault: false,
        manufacturer: 'Jabra',
        deviceType: 'Headset',
        interfaceType: 'Bluetooth',
        address: '48:5F:99:00:11:22',
        driver: 'PipeWire',
        extended: [],
        host: 'pipe-wire',
        transport: 'bluetooth',
        tier: 'primary',
        hint: 'Bluetooth · Headset · Jabra',
      },
      {
        id: 'pipewire:alsa_input.usb-Focusrite_Scarlett_Solo_USB-00.analog-stereo',
        label: 'USB Microphone',
        isDefault: false,
        manufacturer: 'Focusrite',
        deviceType: 'Microphone',
        interfaceType: 'USB',
        address: '1-2',
        driver: 'PipeWire',
        extended: [],
        host: 'pipe-wire',
        transport: 'usb',
        tier: 'primary',
        hint: 'USB · Microphone · Focusrite',
      },
      {
        id: 'pipewire:alsa_input.usb-Logitech_USB_Headset-00.mono-fallback',
        label: 'USB Microphone',
        isDefault: false,
        manufacturer: 'Logitech',
        deviceType: 'Headset',
        interfaceType: 'USB',
        address: '1-3',
        driver: 'PipeWire',
        extended: [],
        host: 'pipe-wire',
        transport: 'usb',
        tier: 'primary',
        hint: 'USB · Headset · Logitech',
      },
      ...advancedDevices,
    ]
  }

  let previewDevices = defaultPreviewDevices()

  function defaultPreviewSystemDefault(): InputDevice {
    return {
      id: 'pipewire:input_default',
      label: 'System default',
      isDefault: true,
      manufacturer: null,
      deviceType: 'Microphone',
      interfaceType: 'Virtual',
      address: null,
      driver: 'PipeWire',
      extended: [],
      host: 'pipe-wire',
      transport: 'virtual',
      tier: 'advanced',
      hint: 'Follows the Linux system default',
    }
  }

  function defaultPreviewMicrophones(devices: InputDevice[]): MicrophoneSnapshot {
    const systemDefault = defaultPreviewSystemDefault()
    return {
      host: 'pipe-wire',
      source: 'default',
      systemDefault,
      systemDefaultIsProxy: true,
      devices,
      selection: { kind: 'system-default', active: systemDefault },
      enumerationWarning: null,
    }
  }

  let previewMicrophones: MicrophoneSnapshot = defaultPreviewMicrophones(previewDevices)

  function getMicrophones(): Promise<MicrophoneSnapshot> {
    return Promise.resolve(previewMicrophones)
  }

  function setMicrophone(id: string | null): Promise<MicrophoneSnapshot> {
    const device = id == null ? null : previewDevices.find((candidate) => candidate.id === id)
    if (id != null && !device) return Promise.reject(new Error('microphone is no longer connected'))
    previewMicrophones = {
      ...previewMicrophones,
      source: id == null ? 'default' : 'config',
      selection:
        device == null
          ? { kind: 'system-default', active: previewMicrophones.systemDefault }
          : { kind: 'selected', device },
    }
    const microphoneReady = device != null || previewMicrophones.systemDefault != null
    previewReadiness = {
      ...previewReadiness,
      microphoneReady,
      firstRunComplete:
        microphoneReady && previewReadiness.speechReady && previewReadiness.hasSuccessfulDictation,
    }
    return Promise.resolve(previewMicrophones)
  }

  function previewMicrophoneTest(device: InputDevice | null): MicrophoneTestResult {
    if (previewMicTestError) {
      return { kind: 'failed', device, category: 'busy', message: previewMicTestError }
    }
    if (!device) {
      return { kind: 'failed', device: null, category: 'disconnected', message: 'microphone is unavailable' }
    }
    const peakRms = device.id.includes('Logitech') ? 0 : 0.042
    return {
      kind: 'completed',
      device,
      peakRms,
      outcome: peakRms > 0.001 ? 'heard' : 'silent',
    }
  }

  function testInputDevice(id: string | null): Promise<MicrophoneTestResult> {
    const device = id == null
      ? previewMicrophones.systemDefault
      : previewDevices.find((candidate) => candidate.id === id) ?? null
    return Promise.resolve(previewMicrophoneTest(device))
  }

  function testMicrophoneFallback(): Promise<MicrophoneTestResult> {
    const fallback = previewMicrophones.selection.kind === 'missing-with-fallback'
      ? previewMicrophones.selection.fallback
      : previewMicrophones.selection.kind === 'ambiguous-legacy-name'
        ? previewMicrophones.selection.fallback
        : previewMicrophones.systemDefault
    return Promise.resolve(
      previewMicrophoneTest(fallback),
    )
  }

  function seedPreviewMicrophones(snapshot: MicrophoneSnapshot) {
    previewDevices = snapshot.devices
    previewMicrophones = snapshot
  }

  function defaultPreviewReadiness(): Readiness {
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
      components: sources.map(({ id, label, path, origin }) => ({
        id,
        label,
        managed: { kind: 'absent', resumableBytes: 0 },
        external: path ? [{ origin, path }] : [],
        activeOrigin: path ? origin : null,
        activity: null,
      })),
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

  let previewReadiness = defaultPreviewReadiness()
  const previewSetupListeners = new Set<(event: SetupEvent) => void>()
  const previewSetupTimers = new Map<string, number>()

  function getReadiness(): Promise<Readiness> {
    return Promise.resolve(previewReadiness)
  }

  function startSetup(plan: SetupPlanId, managedCopy = false): Promise<string> {
    void managedCopy
    const operationId = `preview-${plan}`
    previewReadiness = { ...previewReadiness, activeOperation: operationId, activeCancellable: true }
    const selected = previewReadiness.plans.find((candidate) => candidate.id === plan)
    const component = selected?.components[0] ?? 'whisper-runtime'
    previewSetupListeners.forEach((listener) => listener({
      kind: 'progress',
      progress: { operationId, component, phase: 'downloading', receivedBytes: 1, totalBytes: 2, resumedFromBytes: 0 },
    }))
    const timer = schedulePreview(() => {
      previewSetupTimers.delete(operationId)
      previewReadiness = {
        ...previewReadiness,
        speechReady: true,
        activeOperation: null,
        activeCancellable: false,
        plans: previewReadiness.plans.map((candidate) =>
          candidate.id === plan ? { ...candidate, satisfied: true } : candidate,
        ),
      }
      previewSetupListeners.forEach((listener) => listener({ kind: 'finished', operationId }))
    }, 20)
    previewSetupTimers.set(operationId, timer)
    return Promise.resolve(operationId)
  }

  function repairManaged(component: ComponentId): Promise<string> {
    void component
    return startSetup('recommended', true)
  }

  function verifyManaged(component: ComponentId): Promise<string> {
    void component
    return startSetup('recommended', true)
  }

  function removeManaged(component: ComponentId): Promise<string> {
    previewReadiness = {
      ...previewReadiness,
      components: previewReadiness.components.map((candidate) =>
        candidate.id === component
          ? { ...candidate, managed: { kind: 'absent', resumableBytes: 0 } }
          : candidate,
      ),
    }
    return Promise.resolve(`remove-${component}`)
  }

  function cancelSetup(operation: string): Promise<boolean> {
    const timer = previewSetupTimers.get(operation)
    if (timer != null) {
      clearPreviewTimer(timer)
      previewSetupTimers.delete(operation)
    }
    previewReadiness = { ...previewReadiness, activeOperation: null, activeCancellable: false }
    previewSetupListeners.forEach((listener) => listener({ kind: 'cancelled', operationId: operation }))
    return Promise.resolve(true)
  }

  function onSetupEvent(handler: (event: SetupEvent) => void): Promise<() => void> {
    previewSetupListeners.add(handler)
    return Promise.resolve(() => previewSetupListeners.delete(handler))
  }

  function seedPreviewReadiness(readiness: Readiness) {
    previewReadiness = readiness
  }

  function defaultPreviewSettings(): Settings {
    return projectPreviewSettings({
      engine: { value: null, effective: 'auto', source: 'default' },
      whisperModel: { value: null, effective: '', source: 'default' },
      cleanup: { value: null, effective: 'rules', source: 'default' },
      hud: { value: null, effective: true, source: 'default' },
      recordSeconds: {
        value: null,
        effective: PREVIEW_RECORDING_POLICY.defaultSeconds,
        source: 'default',
      },
      language: { value: null, effective: 'auto', source: 'default' },
      whisperAcceleration: { value: null, effective: 'cpu', source: 'default' },
      whisperGpuDevice: { value: null, effective: '', source: 'default' },
    })
  }

  function projectPreviewSettings(settings: Settings): Settings {
    const recordValue =
      settings.recordSeconds.value == null
        ? null
        : Math.min(
            PREVIEW_RECORDING_POLICY.maximumSeconds,
            Math.max(PREVIEW_RECORDING_POLICY.minimumSeconds, settings.recordSeconds.value),
          )
    return {
      engine: previewField(settings.engine.value, 'auto'),
      whisperModel: previewField(settings.whisperModel.value, ''),
      cleanup: previewField(settings.cleanup.value, 'rules'),
      hud: previewField(settings.hud.value, true),
      recordSeconds: previewField(recordValue, PREVIEW_RECORDING_POLICY.defaultSeconds),
      language: previewField(settings.language.value, 'auto'),
      whisperAcceleration: previewField(settings.whisperAcceleration.value, 'cpu'),
      whisperGpuDevice: previewField(settings.whisperGpuDevice.value, ''),
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
      recordingLimitSeconds: previewStatus.recording
        ? previewStatus.recordingLimitSeconds
        : settings.recordSeconds.effective,
    }
  }

  return {
    richPreviewStatus,
    seedPreviewInventory,
    getAppStatus,
    getShortcutStatus,
    retryShortcut,
    repairLegacyShortcut,
    getHistory,
    getDictionary,
    addDictionaryEntry,
    removeDictionaryEntry,
    toggleRecording,
    stopRecording,
    getRecordingLevel,
    copyText,
    seedPreviewRemoveStaleError,
    removeStaleInstalls,
    getSettings,
    listModels,
    listLanguages,
    seedPreviewLanguages,
    seedPreviewLanguagesError,
    setSettings,
    seedPreviewSettings,
    seedPreviewMicTestError,
    listGpuDevices,
    seedPreviewGpuDevices,
    seedPreviewStatus,
    resetPreviewSettings,
    getMicrophones,
    setMicrophone,
    testInputDevice,
    testMicrophoneFallback,
    seedPreviewMicrophones,
    getReadiness,
    startSetup,
    repairManaged,
    verifyManaged,
    removeManaged,
    cancelSetup,
    onSetupEvent,
    seedPreviewReadiness,
  }
}
