import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../App'
import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import {
  addDictionaryEntry,
  configureDesktopApi,
  clearHistory,
  copyText,
  deleteHistoryItem,
  getAppStatus,
  getHistory,
  getRecordingLevel,
  getShortcutStatus,
  getSettings,
  getMicrophones,
  getReadiness,
  listGpuDevices,
  listLanguages,
  listModels,
  quitApp,
  repairLegacyShortcut,
  retryShortcut,
  repairManaged,
  onSetupEvent,
  setSettings,
  setMicrophone,
  startSetup,
  stopRecording,
  testInputDevice,
  testMicrophoneFallback,
} from '../tauri'
import { deferred } from '../test/desktopApiHarness'
import type {
  ComponentId,
  ComponentStatus,
  LanguageOptions,
  ResolvedSpeechEngine,
  SettingsChange,
  SetupEvent,
  SetupPlanId,
} from '../generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const {
  resetPreviewSettings,
  richPreviewStatus,
  seedPreviewGpuDevices,
  seedPreviewInventory,
  seedPreviewLanguages,
  seedPreviewLanguagesError,
  seedPreviewMicrophones,
  seedPreviewMicTestError,
  seedPreviewReadiness,
  seedPreviewSettings,
  seedPreviewStatus,
} = previewDesktopApi

async function getPreferences() {
  return (await getSettings()).preferences
}

function requireFixture<T>(value: T | null | undefined, description: string): T {
  if (value == null) throw new Error(`missing test fixture: ${description}`)
  return value
}

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    addDictionaryEntry: vi.fn((spoken: string, written: string) =>
      actual.addDictionaryEntry(spoken, written)),
    clearHistory: vi.fn(() => Promise.resolve(0)),
    copyText: vi.fn((text: string) => actual.copyText(text)),
    deleteHistoryItem: vi.fn(() => Promise.resolve(false)),
    getAppStatus: vi.fn(() => actual.getAppStatus()),
    getHistory: vi.fn(() => actual.getHistory()),
    getRecordingLevel: vi.fn(() => actual.getRecordingLevel()),
    getShortcutStatus: vi.fn(() => actual.getShortcutStatus()),
    getSettings: vi.fn(() => actual.getSettings()),
    getMicrophones: vi.fn(() => actual.getMicrophones()),
    getReadiness: vi.fn(() => actual.getReadiness()),
    listGpuDevices: vi.fn((refresh?: boolean) => actual.listGpuDevices(refresh)),
    listLanguages: vi.fn(() => actual.listLanguages()),
    listModels: vi.fn(() => actual.listModels()),
    quitApp: vi.fn(() => actual.quitApp()),
    setSettings: vi.fn((settings: SettingsChange) => actual.setSettings(settings)),
    setMicrophone: vi.fn((id: string | null) => actual.setMicrophone(id)),
    startSetup: vi.fn((plan: SetupPlanId, managedCopy?: boolean) => actual.startSetup(plan, managedCopy)),
    removeStaleInstalls: vi.fn(() => actual.removeStaleInstalls()),
    repairLegacyShortcut: vi.fn(() => actual.repairLegacyShortcut()),
    repairManaged: vi.fn((component: ComponentId) => actual.repairManaged(component)),
    onSetupEvent: vi.fn((handler: (event: SetupEvent) => void) => actual.onSetupEvent(handler)),
    retryShortcut: vi.fn(() => actual.retryShortcut()),
    stopRecording: vi.fn((activation: string) => actual.stopRecording(activation)),
    testInputDevice: vi.fn((id: string | null) => actual.testInputDevice(id)),
    testMicrophoneFallback: vi.fn(() => actual.testMicrophoneFallback()),
  }
})

const nextRunEngineCases: Record<ResolvedSpeechEngine['kind'], {
  engine: ResolvedSpeechEngine
  label: string
  processing: string
}> = {
  whisper: {
    engine: { kind: 'whisper', model: 'small', multilingual: true },
    label: 'Whisper · small · EN',
    processing: 'CPU',
  },
  parakeet: {
    engine: { kind: 'parakeet', model: 'tdt-0.6b-v3' },
    label: 'Parakeet · tdt-0.6b-v3 · EN',
    processing: 'Engine-managed processing',
  },
  fake: {
    engine: { kind: 'fake' },
    label: 'Fake test engine · EN',
    processing: 'Engine-managed processing',
  },
}

describe('Echo desktop shell', () => {
  beforeEach(async () => {
    vi.restoreAllMocks()
    configureDesktopApi(previewDesktopApi)
    resetPreviewSettings()
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    vi.mocked(addDictionaryEntry).mockReset()
    vi.mocked(addDictionaryEntry).mockImplementation((spoken, written) =>
      actual.addDictionaryEntry(spoken, written))
    vi.mocked(clearHistory).mockReset()
    vi.mocked(clearHistory).mockResolvedValue(3)
    vi.mocked(copyText).mockReset()
    vi.mocked(copyText).mockImplementation((text) => actual.copyText(text))
    vi.mocked(deleteHistoryItem).mockReset()
    vi.mocked(deleteHistoryItem).mockResolvedValue(true)
    vi.mocked(getAppStatus).mockReset()
    vi.mocked(getAppStatus).mockImplementation(() => actual.getAppStatus())
    vi.mocked(getHistory).mockReset()
    vi.mocked(getHistory).mockImplementation(() => actual.getHistory())
    vi.mocked(getRecordingLevel).mockReset()
    vi.mocked(getRecordingLevel).mockImplementation(() => actual.getRecordingLevel())
    vi.mocked(getShortcutStatus).mockReset()
    vi.mocked(getShortcutStatus).mockImplementation(() => actual.getShortcutStatus())
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((settings) => actual.setSettings(settings))
    vi.mocked(repairLegacyShortcut).mockReset()
    vi.mocked(repairLegacyShortcut).mockImplementation(() => actual.repairLegacyShortcut())
    vi.mocked(retryShortcut).mockReset()
    vi.mocked(retryShortcut).mockImplementation(() => actual.retryShortcut())
    vi.mocked(stopRecording).mockReset()
    vi.mocked(stopRecording).mockImplementation((activation) => actual.stopRecording(activation))
    vi.mocked(getSettings).mockReset()
    vi.mocked(getSettings).mockImplementation(() => actual.getSettings())
    vi.mocked(getMicrophones).mockReset()
    vi.mocked(getMicrophones).mockImplementation(() => actual.getMicrophones())
    vi.mocked(getReadiness).mockReset()
    vi.mocked(getReadiness).mockImplementation(() => actual.getReadiness())
    vi.mocked(listGpuDevices).mockReset()
    vi.mocked(listGpuDevices).mockImplementation((refresh) => actual.listGpuDevices(refresh))
    vi.mocked(listLanguages).mockReset()
    vi.mocked(listLanguages).mockImplementation(() => actual.listLanguages())
    vi.mocked(setMicrophone).mockReset()
    vi.mocked(setMicrophone).mockImplementation((id) => actual.setMicrophone(id))
    vi.mocked(startSetup).mockReset()
    vi.mocked(startSetup).mockImplementation((plan, managedCopy) => actual.startSetup(plan, managedCopy))
    vi.mocked(listModels).mockReset()
    vi.mocked(listModels).mockImplementation(() => actual.listModels())
    vi.mocked(quitApp).mockReset()
    vi.mocked(quitApp).mockImplementation(() => actual.quitApp())
    vi.mocked(onSetupEvent).mockReset()
    vi.mocked(onSetupEvent).mockImplementation((handler) => actual.onSetupEvent(handler))
    vi.mocked(testInputDevice).mockReset()
    vi.mocked(testInputDevice).mockImplementation((id) => actual.testInputDevice(id))
    vi.mocked(testMicrophoneFallback).mockReset()
    vi.mocked(testMicrophoneFallback).mockImplementation(() => actual.testMicrophoneFallback())
  })

  it('unlistens when the Settings subscription setup resolves after navigation', async () => {
    const pending = deferred<() => void>()
    const unlisten = vi.fn()
    vi.mocked(onSetupEvent)
      .mockResolvedValueOnce(vi.fn())
      .mockImplementationOnce(() => pending.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await waitFor(() => expect(onSetupEvent).toHaveBeenCalledTimes(2))

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    pending.resolve(unlisten)

    await waitFor(() => expect(unlisten).toHaveBeenCalledOnce())
  })

  it('ignores Settings loader failures that settle after navigation', async () => {
    const settings = deferred<Awaited<ReturnType<typeof getSettings>>>()
    vi.mocked(getSettings).mockImplementation(() => settings.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledOnce())

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    await act(async () => {
      settings.reject(new Error('late settings failure'))
      await Promise.allSettled([settings.promise])
    })

    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })


  it('ignores GPU enumeration failure after Settings unmounts', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const settings = (await actual.getSettings()).preferences
    const readiness = await actual.getReadiness()
    const installed: ComponentStatus['managed'] = {
      kind: 'ready',
      version: 'test',
      bytes: 100,
      root: '/managed',
    }
    seedPreviewSettings({
      ...settings,
      whisperAcceleration: { value: 'gpu', effective: 'gpu', source: 'file' },
    })
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'whisper-runtime' || component.id === 'whisper-vulkan-runtime'
          ? { ...component, managed: installed, activeOrigin: 'managed' }
          : component,
      ),
    })
    const devices = deferred<Awaited<ReturnType<typeof listGpuDevices>>>()
    vi.mocked(listGpuDevices).mockImplementation(() => devices.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await waitFor(() => expect(listGpuDevices).toHaveBeenCalledOnce())

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    await act(async () => {
      devices.reject(new Error('late GPU enumeration failure'))
      await devices.promise.catch(() => undefined)
    })

    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('ignores a microphone test failure after Settings unmounts', async () => {
    const test = deferred<Awaited<ReturnType<typeof testInputDevice>>>()
    vi.mocked(testInputDevice).mockImplementation(() => test.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test selected' }))
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))

    await act(async () => {
      test.reject(new Error('late microphone test failure'))
      await test.promise.catch(() => undefined)
    })

    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('shows a rejected microphone test and clears its busy state', async () => {
    vi.mocked(testInputDevice).mockRejectedValueOnce(new Error('microphone test crashed'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const testButton = await screen.findByRole('button', { name: 'Test selected' })

    fireEvent.click(testButton)

    expect(await screen.findByRole('alert')).toHaveTextContent('microphone test crashed')
    await waitFor(() => expect(testButton).toBeEnabled())
  })

  it('writes a settings change and renders the stored value', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' }))
    expect(within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' })).toHaveAttribute('data-active', 'true')
    expect((await getPreferences()).hud).toEqual({ value: false, effective: false, source: 'file' })
  })

  it('keeps aria-pressed aligned with every segmented control selection', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const groups = await Promise.all([
      screen.findByRole('group', { name: 'Speech engine' }),
      screen.findByRole('group', { name: 'Whisper acceleration' }),
      screen.findByRole('group', { name: 'Recording HUD' }),
      screen.findByRole('group', { name: 'Application theme' }),
    ])
    for (const group of groups) {
      for (const button of within(group).getAllByRole('button')) {
        expect(button.getAttribute('aria-pressed')).toBe(button.getAttribute('data-active'))
      }
    }
  })

  it('offers the Rust recording policy under Input and clears the default override', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const select = await screen.findByLabelText('Maximum recording length')
    expect(within(select).getAllByRole('option').map((option) => option.textContent)).toEqual([
      '30 seconds',
      '1 minute',
      '2 minutes',
      '5 minutes',
      '10 minutes · Default',
    ])
    expect(select).toHaveValue('default')

    fireEvent.change(select, { target: { value: '120' } })
    await waitFor(async () => expect((await getPreferences()).recordSeconds.value).toBe(120))
    fireEvent.change(select, { target: { value: 'default' } })
    await waitFor(async () => expect((await getPreferences()).recordSeconds.value).toBeNull())
  })

  it('preserves a custom recording limit and locks an environment override', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: 90, effective: 90, source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('option', { name: '90 seconds' })).toBeInTheDocument()
    expect(screen.getByLabelText('Maximum recording length')).toHaveValue('90')

    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: 30, effective: 90, source: 'env' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const locked = await screen.findByLabelText('Maximum recording length')
    expect(locked).toBeDisabled()
    expect(locked).toHaveValue('90')
    expect(screen.getByText('ECHO_RECORD_SECONDS')).toBeInTheDocument()
  })

  it('shows an explicit 600-second environment override as selected', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: null, effective: 600, source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const select = await screen.findByLabelText('Maximum recording length')
    expect(select).toHaveValue('600')
    expect(within(select).getByRole('option', { name: '10 minutes' })).toBeInTheDocument()
  })


  it('disables an env-backed field and names the variable', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      hud: { value: null, effective: false, source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const off = within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' })
    expect(off).toBeDisabled()
    expect(off).toHaveAttribute('data-active', 'true')
    expect(screen.getByText('ECHO_HUD')).toBeInTheDocument()
    fireEvent.click(within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'On' }))
    expect((await getPreferences()).hud.effective).toBe(false)
  })

  it('persists two rapid settings writes', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    let releaseFirst = () => {}
    let signalFirstStarted = () => {}
    const firstWriteStarted = new Promise<void>((resolve) => {
      signalFirstStarted = resolve
    })
    const firstWriteGate = new Promise<void>((resolve) => {
      releaseFirst = resolve
    })
    let writes = 0
    vi.mocked(setSettings).mockImplementation(async (settings) => {
      writes += 1
      if (writes === 1) {
        signalFirstStarted()
        await firstWriteGate
      }
      return actual.setSettings(settings)
    })

    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.change(await screen.findByLabelText('Maximum recording length'), { target: { value: '30' } })
    await firstWriteStarted
    fireEvent.click(within(screen.getByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' }))
    releaseFirst()

    await waitFor(async () => {
      const stored = await getPreferences()
      expect(stored.recordSeconds).toEqual({ value: 30, effective: 30, source: 'file' })
      expect(stored.hud).toEqual({ value: false, effective: false, source: 'file' })
    })
  })

  it('shows the error banner when a settings save fails', async () => {
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('could not write settings'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const hud = await screen.findByRole('group', { name: 'Recording HUD' })
    fireEvent.click(within(hud).getByRole('button', { name: 'Off' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('could not write settings')
    expect(within(hud).getByRole('button', { name: 'Off' })).toHaveAttribute('data-active', 'false')
  })


  it('renders duplicate labels as distinct stable-ID radio rows', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const choices = await screen.findAllByRole('radio', { name: /USB Microphone/ })
    expect(choices).toHaveLength(2)
    expect(screen.getByText('USB · Microphone · Focusrite')).toBeInTheDocument()
    expect(screen.getByText('USB · Headset · Logitech')).toBeInTheDocument()
    fireEvent.click(requireFixture(choices[1], 'second USB microphone choice'))
    await waitFor(async () => {
      const snapshot = await getMicrophones()
      expect(snapshot.selection).toMatchObject({
        kind: 'selected',
        device: { id: 'pipewire:alsa_input.usb-Logitech_USB_Headset-00.mono-fallback' },
      })
    })
  })

  it('shows recognizable sources before collapsed technical endpoints', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findByRole('radio', { name: /Jabra Elite 8 Active/ })).toBeVisible()
    expect(screen.getByText('Bluetooth · Headset · Jabra')).toBeVisible()
    expect(screen.getByText('Follows the current Linux input automatically')).toBeVisible()
    expect(screen.queryByText('pipewire:input_default')).not.toBeInTheDocument()
    const advanced = requireFixture(
      screen.getByText('Advanced audio endpoints').closest('details'),
      'Advanced audio endpoints disclosure',
    )
    expect(advanced).not.toHaveAttribute('open')
    expect(screen.getByText('PipeWire Sound Server')).not.toBeVisible()
    fireEvent.click(screen.getByText('Advanced audio endpoints'))
    expect(screen.getByText('PipeWire Sound Server')).toBeVisible()
    expect(screen.getByText('alsa:pipewire')).toBeVisible()
  })

  it('names the active input when Linux has no declared default', async () => {
    const snapshot = await getMicrophones()
    const active = requireFixture(snapshot.devices[0], 'active microphone device')
    seedPreviewMicrophones({
      ...snapshot,
      systemDefault: null,
      systemDefaultIsProxy: false,
      selection: { kind: 'system-default', active },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText(`Using ${active.label} because Linux has no default input`)).toBeVisible()
  })

  it('names a missing selection and tests fallback only through the explicit action', async () => {
    const snapshot = await getMicrophones()
    seedPreviewMicrophones({
      ...snapshot,
      source: 'config',
      selection: {
        kind: 'missing-with-fallback',
        requestedId: 'alsa:travel',
        requestedLabel: 'Travel Mic',
        fallback: requireFixture(snapshot.systemDefault, 'system default microphone'),
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText(/Travel Mic is disconnected/)).toBeInTheDocument()
    expect(screen.getByText(/current input from Linux Sound Settings/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Test selected' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Test system fallback' }))
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Input heard on System default',
    )
  })

  it('clears the microphone test result when the selection changes', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test selected' }))
    expect(await screen.findByRole('status')).toHaveTextContent('Input heard')
    const choices = await screen.findAllByRole('radio', { name: /USB Microphone/ })
    fireEvent.click(requireFixture(choices[0], 'first USB microphone choice'))
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
  })

  it('announces the exact microphone test failure', async () => {
    seedPreviewMicTestError('device busy')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test selected' }))
    expect(await screen.findByRole('status')).toHaveTextContent('device busy')
  })

  it('hides the Fake engine from the selector by default', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByRole('button', { name: 'Whisper' })
    expect(screen.queryByRole('button', { name: 'Fake' })).not.toBeInTheDocument()
  })

  it('shows the Fake engine when the availability payload includes it', async () => {
    const { listModels } = await import('../tauri')
    const inventory = await listModels()
    seedPreviewInventory({
      ...inventory,
      engines: [...inventory.engines, { id: 'fake', available: true, reason: null }],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('button', { name: 'Fake' })).toBeInTheDocument()
  })

  it('shows the Whisper picker and the fixed Parakeet speech model', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    // Default fixture: engine auto with Whisper available, so the picker shows.
    const picker = await screen.findByLabelText('Speech model')
    expect(screen.getByRole('option', { name: 'Automatic · currently small' })).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'Recommended for fast dictation · small · multilingual · full precision · 466 MiB' }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'Higher accuracy · large-v3-turbo-q8_0 · multilingual · q8_0 · 834 MiB' }),
    ).toBeInTheDocument()

    fireEvent.change(picker, { target: { value: 'small' } })
    await waitFor(async () => {
      expect((await getPreferences()).whisperModel).toEqual({
        value: 'small',
        effective: 'small',
        source: 'file',
      })
    })

    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    expect(screen.getByText(/GPU is a preference with automatic CPU fallback/)).toBeInTheDocument()
    expect(acceleration.queryByRole('button', { name: 'Automatic' })).not.toBeInTheDocument()
    expect(acceleration.getByRole('button', { name: 'CPU' })).toBeInTheDocument()
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))
    await waitFor(async () => {
      expect((await getPreferences()).whisperAcceleration).toEqual({
        value: 'gpu',
        effective: 'gpu',
        source: 'file',
      })
    })
    fireEvent.click(await screen.findByRole('button', { name: 'Parakeet' }))
    await waitFor(() => expect(screen.queryByLabelText('Speech model')).not.toBeInTheDocument())
    expect(screen.getByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    await waitFor(async () => expect((await getPreferences()).whisperModel.value).toBeNull())
  })

  it('keeps the Whisper model picker available when the saved model is missing', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'whisper', effective: 'whisper', source: 'file' },
      whisperModel: { value: 'missing-model', effective: 'missing-model', source: 'file' },
    })

    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findByText('Whisper model missing-model is not installed')).toBeInTheDocument()
    const picker = screen.getByLabelText('Speech model')
    expect(picker).toHaveValue('missing-model')
    expect(within(picker).getByRole('option', { name: 'missing-model · not on disk' }))
      .toBeInTheDocument()

    fireEvent.change(picker, { target: { value: 'small' } })
    await waitFor(() => expect(screen.getByText('Whisper · small · Automatic language')).toBeInTheDocument())
  })

  it('projects Auto as Parakeet when the backend language mode resolves Parakeet', async () => {
    seedPreviewLanguages({
      mode: 'parakeet',
      model: null,
      options: Array.from({ length: 25 }, (_, index) => ({
        code: `p${index}`,
        englishName: `parakeet language ${index}`,
        group: 'all',
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    expect(screen.queryByLabelText('Speech model')).not.toBeInTheDocument()
  })

  it('keeps a saved GPU preference dormant while the next run resolves Parakeet', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      whisperAcceleration: { value: 'gpu', effective: 'gpu', source: 'file' },
    })
    seedPreviewLanguages({
      mode: 'parakeet',
      model: 'tdt-0.6b-v3',
      options: Array.from({ length: 25 }, (_, index) => ({
        code: `p${index}`,
        englishName: `parakeet language ${index}`,
        group: 'all',
      })),
    })

    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findByText('GPU preference saved for Whisper')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Install Whisper/ })).not.toBeInTheDocument()
    expect(listGpuDevices).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Use Whisper with GPU' }))
    await waitFor(async () => {
      const preferences = await getPreferences()
      expect(preferences.engine.effective).toBe('whisper')
      expect(preferences.whisperAcceleration.effective).toBe('gpu')
    })
    expect(
      within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
        .getByRole('button', { name: 'GPU' }),
    ).toHaveAttribute('data-active', 'true')
    expect(await screen.findByRole('button', { name: /Install Whisper/ })).toBeInTheDocument()
  })

  it.each([
    {
      label: 'engine',
      expectedVariable: 'ECHO_ENGINE',
      engine: { value: null, effective: 'parakeet', source: 'env' as const },
      acceleration: { value: 'gpu', effective: 'gpu', source: 'file' as const },
    },
    {
      label: 'acceleration',
      expectedVariable: 'ECHO_WHISPER_ACCELERATION',
      engine: { value: 'parakeet', effective: 'parakeet', source: 'file' as const },
      acceleration: { value: 'gpu', effective: 'cpu', source: 'env' as const },
    },
  ])('does not offer a GPU transition blocked by an $label override', async ({
    expectedVariable,
    engine,
    acceleration,
  }) => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      engine,
      whisperAcceleration: acceleration,
    })
    seedPreviewLanguages({
      mode: 'parakeet',
      model: 'tdt-0.6b-v3',
      options: [{ code: 'en', englishName: 'english', group: 'all' }],
    })

    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findAllByText(expectedVariable)).not.toHaveLength(0)
    expect(screen.queryByRole('button', { name: 'Use Whisper with GPU' })).not.toBeInTheDocument()
  })

  it('honors a backend Whisper projection over a file-backed Parakeet choice', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'parakeet', effective: 'parakeet', source: 'file' },
      whisperModel: { value: null, effective: 'small', source: 'env' },
    })
    seedPreviewLanguages({
      mode: 'multilingual',
      model: 'small',
      options: [{ code: 'en', englishName: 'english', group: 'common' }],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByLabelText('Speech model')).toBeDisabled()
    expect(screen.queryByText('Parakeet TDT 0.6B v3')).not.toBeInTheDocument()
  })

  it('keeps the last coherent snapshot when a settings refresh fails', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Speech model')
    seedPreviewLanguagesError('language projection failed')
    const parakeet = await screen.findByRole('button', { name: 'Parakeet' })
    fireEvent.click(parakeet)
    expect(await screen.findByText('language projection failed')).toBeInTheDocument()
    expect(parakeet).toHaveAttribute('data-active', 'false')
    expect(screen.getByLabelText('Speech model')).toBeInTheDocument()
  })

  it('renders task-based Settings sections without a top-level Advanced drawer', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    await screen.findByLabelText('Microphone')
    await screen.findByLabelText('Language')
    await screen.findByLabelText('Speech model')
    expect(await screen.findAllByText('Super+Alt+Space')).not.toHaveLength(0)
    expect(screen.getByRole('group', { name: 'Application theme' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeInTheDocument()

    expect(screen.getByRole('region', { name: 'Transcription' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Input and controls' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Appearance' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Setup and diagnostics' })).toBeInTheDocument()
    expect(document.querySelector('.advanced-section')).not.toBeInTheDocument()
    expect(screen.queryByText('Advanced', { exact: true })).not.toBeInTheDocument()
    expect(await screen.findByRole('group', { name: 'Speech engine' })).toBeInTheDocument()
    expect(screen.getByText('Next transcription')).toBeInTheDocument()
    expect(screen.getByText('Previous transcription')).toBeInTheDocument()
  })

  it.each(Object.values(nextRunEngineCases))(
    'renders the $engine.kind next-transcription summary explicitly',
    async ({ engine, label, processing }) => {
      const snapshot = await getSettings()
      vi.mocked(getSettings).mockResolvedValue({
        ...snapshot,
        preferences: {
          ...snapshot.preferences,
          whisperAcceleration: {
            ...snapshot.preferences.whisperAcceleration,
            effective: 'cpu',
          },
        },
        transcription: {
          ...snapshot.transcription,
          nextRun: { kind: 'ready', engine, language: 'en' },
        },
      })

      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

      const summary = (await screen.findByText(label)).closest('.next-run-summary')
      if (!(summary instanceof HTMLElement)) {
        throw new Error('missing next-transcription summary')
      }
      expect(within(summary).getByText(processing)).toBeInTheDocument()
    },
  )

  it('marks an unavailable engine with its reason', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findByText('Parakeet: sherpa-onnx-offline is not on PATH'),
    ).toBeInTheDocument()
  })

  it('renders the last-run readout from the fixture', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('whisper-small · 1038 ms')).toBeInTheDocument()
    expect(screen.getByText('VULKAN · Intel Iris Xe')).toBeInTheDocument()
    expect(screen.getByText(__APP_VERSION__)).toBeInTheDocument()
  })

  // The preview seeds both managed runtimes absent, which is what a real
  // install looks like until someone chooses GPU. Any test about the device
  // picker has to say that it is testing the installed case.
  async function seedGpuRuntime(
    gpu: ComponentStatus['managed'],
    cpu: ComponentStatus['managed'] = installedGpuRuntime,
  ) {
    const readiness = await getReadiness()
    const managedFor = (id: string) =>
      id === 'whisper-vulkan-runtime' ? gpu : id === 'whisper-runtime' ? cpu : null
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) => {
        const managed = managedFor(component.id)
        return managed == null
          ? component
          : { ...component, managed, activeOrigin: managed.kind === 'ready' ? 'managed' : null }
      }),
    })
  }

  const installedGpuRuntime: ComponentStatus['managed'] = {
    kind: 'ready',
    version: '1.9.2-vulkan',
    bytes: 59_816_721,
    root: '/managed',
  }

  it('offers the GPU device picker only when GPU is selected', async () => {
    await seedGpuRuntime(installedGpuRuntime)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(screen.queryByLabelText('GPU device')).not.toBeInTheDocument()

    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))

    const picker = await screen.findByLabelText('GPU device')
    await waitFor(() =>
      expect(
        within(picker).getByRole('option', { name: 'Intel(R) Iris(R) Xe Graphics (ADL GT2)' }),
      ).toBeInTheDocument(),
    )
    expect(within(picker).getByRole('option', { name: 'Automatic' })).toBeInTheDocument()
    expect(
      within(picker).getByRole('option', { name: 'AMD Radeon RX 7800 XT (RADV)' }),
    ).toBeInTheDocument()
    // A software rasterizer is offered but marked, never chosen for you.
    expect(
      within(picker).getByRole('option', { name: 'llvmpipe (LLVM 20.1.8) · software' }),
    ).toBeInTheDocument()
  })

  it('offers to install the GPU runtime instead of blaming the hardware', async () => {
    // Enumeration needs the runtime, so with none installed the picker would
    // report "no Vulkan device detected" on a machine that has one. Selecting
    // GPU has to be able to get the runtime, or the control does nothing.
    await seedGpuRuntime({ kind: 'absent', resumableBytes: 0 })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))

    const install = await screen.findByRole('button', { name: 'Install Whisper GPU runtime' })
    expect(screen.getByText(/GPU needs Whisper GPU runtime/)).toBeInTheDocument()
    expect(screen.queryByText(/No Vulkan device detected/)).not.toBeInTheDocument()
    expect(screen.queryByLabelText('GPU device')).not.toBeInTheDocument()

    fireEvent.click(install)
    await waitFor(() => expect(repairManaged).toHaveBeenCalledWith('whisper-vulkan-runtime'))
  })

  it('asks for the managed CPU runtime the GPU path falls back to', async () => {
    // A system whisper-cli satisfies speech setup, so nothing else reports this
    // as missing, and WhisperPlanDecision::qualified refuses a fallback that is
    // not the managed CPU runtime. Without this the picker would appear, find
    // no devices, and every run would stay on CPU with no reason given.
    await seedGpuRuntime(installedGpuRuntime, { kind: 'absent', resumableBytes: 0 })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))

    const install = await screen.findByRole('button', { name: 'Install Whisper runtime' })
    expect(screen.queryByLabelText('GPU device')).not.toBeInTheDocument()
    fireEvent.click(install)
    await waitFor(() => expect(repairManaged).toHaveBeenCalledWith('whisper-runtime'))
  })

  it('shows the picker rather than an install prompt once the runtime is ready', async () => {
    await seedGpuRuntime(installedGpuRuntime)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))

    const picker = await screen.findByLabelText('GPU device')
    await waitFor(() => expect(within(picker).getAllByRole('option')).toHaveLength(4))
    expect(
      screen.queryByRole('button', { name: 'Install Whisper GPU runtime' }),
    ).not.toBeInTheDocument()
  })

  it('pins the chosen GPU by its device and driver UUID pair', async () => {
    await seedGpuRuntime(installedGpuRuntime)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const acceleration = within(await screen.findByRole('group', { name: 'Whisper acceleration' }))
    fireEvent.click(acceleration.getByRole('button', { name: 'GPU' }))

    const picker = await screen.findByLabelText('GPU device')
    await waitFor(() => expect(within(picker).getAllByRole('option')).toHaveLength(4))
    fireEvent.change(picker, {
      target: { value: '1002744c0000000000010000000000aa:3f7b1c9a45e1e718c6121d36d8340000' },
    })
    await waitFor(async () => {
      expect((await getPreferences()).whisperGpuDevice.value).toBe(
        '1002744c0000000000010000000000aa:3f7b1c9a45e1e718c6121d36d8340000',
      )
    })
  })

  it('reports a pinned GPU that no longer enumerates without reassigning it', async () => {
    const defaults = await getPreferences()
    seedPreviewGpuDevices([])
    seedPreviewSettings({
      ...defaults,
      whisperAcceleration: { value: 'gpu', effective: 'gpu', source: 'file' },
      whisperGpuDevice: {
        value: 'aa'.repeat(16) + ':' + 'bb'.repeat(16),
        effective: 'aa'.repeat(16) + ':' + 'bb'.repeat(16),
        source: 'file',
      },
    })
    await seedGpuRuntime(installedGpuRuntime)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const picker = await screen.findByLabelText('GPU device')
    expect(within(picker).getByRole('option', { name: /not detected/ })).toBeInTheDocument()
    expect(screen.getByText(/pinned device is not detected/)).toBeInTheDocument()
  })

  it('keeps engine internals out of the readout', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByText('whisper-small · 1038 ms')
    for (const label of [
      'Model file',
      'Binary',
      'Multilingual',
      'VAD',
      'Whisper mode',
      'Runtime',
      'Whisper timing',
      'Decoding',
    ]) {
      expect(screen.queryByText(label)).not.toBeInTheDocument()
    }
  })

  it.each([
    ['runtimeMissing', 'GPU asked for, runtime not installed'],
    ['noDeviceEnumerated', 'GPU asked for, no device found'],
    ['pinnedDeviceAbsent', 'GPU asked for, the selected device is absent'],
    ['deviceQuarantined', 'GPU asked for, the device is disabled after a failure'],
    ['cpuFallbackMissing', 'GPU asked for, the managed CPU runtime it falls back to is missing'],
    ['deviceNotReady', 'GPU asked for, the device did not pass its readiness check'],
    ['recoveredToCpu', 'GPU ran and failed, retried on CPU'],
  ] as const)('says why a requested GPU did not run: %s', async (reason, copy) => {
    const status = richPreviewStatus()
    const lastRun = requireFixture(status.lastRun, 'rich preview last run')
    const performance = requireFixture(lastRun.performance, 'rich preview performance')
    seedPreviewStatus({
      lastRun: {
        ...lastRun,
        performance: {
          ...performance,
          backend: 'cpu',
          device: null,
          accelerationSkip: reason,
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText(`CPU · ${copy}`)).toBeInTheDocument()
  })

  it('says nothing about a fallback when the GPU actually ran', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('VULKAN · Intel Iris Xe')).toBeInTheDocument()
  })

  it('surfaces engine stderr on failure', async () => {
    seedPreviewStatus({
      phase: 'Failed',
      lastError: 'whisper-cli: ggml_init failed',
    })
    render(<App />)
    await screen.findByText('Failed')
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('whisper-cli: ggml_init failed')).toBeInTheDocument()
  })

  it('offers Auto first, then a common group, then the alphabetical list', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const picker = await screen.findByLabelText('Language')
    const auto = screen.getByRole('option', { name: 'Auto · detect language' })
    expect(auto).toBeInTheDocument()
    // The common group keeps its fixed order; the full list is alphabetical.
    const common = within(screen.getByRole('group', { name: 'Common' }))
    expect(common.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'English',
      'German',
      'Spanish',
      'French',
    ])
    const all = within(screen.getByRole('group', { name: 'All languages' }))
    const names = all.getAllByRole('option').map((option, index) =>
      requireFixture(option.textContent, `language option text at index ${index}`),
    )
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)))

    fireEvent.change(picker, { target: { value: 'de' } })
    await waitFor(async () => {
      expect((await getPreferences()).language).toEqual({
        value: 'de',
        effective: 'de',
        source: 'file',
      })
    })
  })

  it('renders every language a multilingual model offers', async () => {
    const hundred: LanguageOptions['options'] = Array.from({ length: 100 }, (_, i) => ({
      code: `l${i}`,
      englishName: `language ${String(i).padStart(3, '0')}`,
      group: i < 4 ? 'common' : 'all',
    }))
    seedPreviewLanguages({ mode: 'multilingual', model: null, options: hundred })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Language')
    const all = within(screen.getByRole('group', { name: 'All languages' }))
    expect(all.getAllByRole('option')).toHaveLength(100)
    expect(screen.getByRole('option', { name: 'Auto · detect language' })).toBeInTheDocument()
  })

  it('shows the detected-language chip only when Auto is active', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('de · German · p=0.96')).toBeInTheDocument()
  })

  it('hides the detected-language chip when a language is pinned', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'en', effective: 'en', source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Language')
    expect(screen.queryByText('de · German · p=0.96')).not.toBeInTheDocument()
  })

  it('offers to pin a confidently detected language, and pins it', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    // Default fixture: auto active, last run detected German at p=0.96.
    const pin = await screen.findByRole('button', { name: 'Pin German for speed' })
    fireEvent.click(pin)
    await waitFor(async () => {
      expect((await getPreferences()).language).toEqual({
        value: 'de',
        effective: 'de',
        source: 'file',
      })
    })
  })

  it('keeps the pin suggestion silent on low confidence', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    seedPreviewStatus({
      lastRun: {
        engine: 'whisper-small',
        binary: null,
        modelPath: null,
        multilingual: true,
        vad: false,
        inferMs: 900,
        language: 'nl',
        languageProbability: 0.31,
        performance: null,
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByText('nl · p=0.31')
    expect(screen.queryByRole('button', { name: /Pin .* for speed/ })).not.toBeInTheDocument()
  })

  it('renders low detection confidence differently', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    seedPreviewStatus({
      lastRun: {
        engine: 'whisper-small',
        binary: null,
        modelPath: null,
        multilingual: true,
        vad: false,
        inferMs: 900,
        language: 'nl',
        languageProbability: 0.31,
        performance: null,
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const chip = await screen.findByText('nl · p=0.31')
    expect(chip.closest('.status-note')).toHaveAttribute('data-tone', 'attention')
  })

  it('warns before recording when an English-only model meets a non-English choice', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'whisper', effective: 'whisper', source: 'file' },
      language: { value: 'de', effective: 'de', source: 'file' },
    })
    seedPreviewLanguages({ mode: 'english', model: 'ggml-base.en.bin', options: [
      { code: 'en', englishName: 'english', group: 'common' },
    ] })
    seedPreviewStatus({
      languageWarning:
        'ggml-base.en.bin is English-only but the language is set to german. Choose a multilingual model or set the language to English.',
    })
    render(<App />)
    // The warning is on Home before any recording happens.
    expect(
      await screen.findByText(/ggml-base\.en\.bin is English-only but the language is set to german/),
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findAllByText(/ggml-base\.en\.bin is English-only/),
    ).not.toHaveLength(0)
    expect(screen.getByLabelText('Speech model')).toBeInTheDocument()
    // An English-only model offers no language picker.
    expect(screen.queryByLabelText('Language')).not.toBeInTheDocument()
  })

  it('reports Parakeet as an automatic fixed speech model', async () => {
    const defaults = await getPreferences()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'parakeet', effective: 'parakeet', source: 'file' },
    })
    seedPreviewLanguages({
      mode: 'parakeet',
      model: null,
      options: Array.from({ length: 25 }, (_, i) => ({
        code: `p${i}`,
        englishName: `parakeet language ${i}`,
        group: 'all',
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findByText('Automatic across 25 languages · not reported'),
    ).toBeInTheDocument()
    expect(screen.getByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    expect(screen.queryByLabelText('Language')).not.toBeInTheDocument()
  })
})
