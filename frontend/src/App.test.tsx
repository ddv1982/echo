import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import process from 'node:process'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { createPreviewDesktopApi } from './api/previewDesktopApi'
import { useSerialPoll } from './hooks/useSerialPoll'
import {
  addDictionaryEntry,
  configureDesktopApi,
  clearHistory,
  copyText,
  deleteHistoryItem,
  getAppStatus,
  getRecordingLevel,
  getShortcutStatus,
  getSettings,
  getMicrophones,
  getReadiness,
  listGpuDevices,
  listLanguages,
  listModels,
  quitApp,
  removeStaleInstalls,
  repairLegacyShortcut,
  retryShortcut,
  repairManaged,
  onSetupEvent,
  setSettings,
  setMicrophone,
  stopRecording,
  testInputDevice,
  testMicrophoneFallback,
} from './tauri'
import type {
  AppStatus,
  ComponentId,
  ComponentStatus,
  LanguageOptions,
  ResolvedSpeechEngine,
  SettingsChange,
  SetupEvent,
  ShortcutStatus,
} from './generated/ipc'

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
  seedPreviewRemoveStaleError,
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

vi.mock('./tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./tauri')>()
  return {
    ...actual,
    addDictionaryEntry: vi.fn((spoken: string, written: string) =>
      actual.addDictionaryEntry(spoken, written)),
    clearHistory: vi.fn(() => Promise.resolve(0)),
    copyText: vi.fn((text: string) => actual.copyText(text)),
    deleteHistoryItem: vi.fn(() => Promise.resolve(false)),
    getAppStatus: vi.fn(() => actual.getAppStatus()),
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

function deferred<T>() {
  const state: {
    resolve: ((value: T | PromiseLike<T>) => void) | null
    reject: ((reason?: unknown) => void) | null
  } = { resolve: null, reject: null }
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    state.resolve = resolvePromise
    state.reject = rejectPromise
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!state.resolve) throw new Error('deferred promise is not initialized')
      state.resolve(value)
    },
    reject(reason?: unknown) {
      if (!state.reject) throw new Error('deferred promise is not initialized')
      state.reject(reason)
    },
  }
}

function SerialPollHarness({
  request,
  onResult,
}: {
  request: () => Promise<number>
  onResult: (result: number) => void
}) {
  useSerialPoll({ request, onResult, intervalMs: 400 })
  return null
}

function activeShortcut(
  activation: string | null = null,
  effective = 'Super+Alt+Space',
): ShortcutStatus {
  return {
    kind: 'active',
    desired: 'Super+Alt+Space',
    effective,
    backend: 'portal',
    activation,
    verificationIdentity: `portal:${effective}`,
  }
}

function shortcutActivation(recordingToken: string, at = wallClockNow()) {
  const seconds = Math.floor(at / 1_000)
  const nanoseconds = Math.floor((at - seconds * 1_000) * 1_000_000)
  return `native-toggle:${seconds}:${nanoseconds}:123:1:recording=${recordingToken}`
}

function wallClockNow() {
  return performance.timeOrigin + performance.now()
}

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
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
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

  it('waits for a status request to settle before polling again', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const pending = deferred<AppStatus>()
    vi.mocked(getAppStatus).mockImplementation(() => pending.promise)
    try {
      render(<App />)
      await waitFor(() => expect(getAppStatus).toHaveBeenCalledOnce())
      await vi.advanceTimersByTimeAsync(1_200)
      expect(getAppStatus).toHaveBeenCalledOnce()
      pending.resolve(richPreviewStatus())
      await act(async () => pending.promise)
    } finally {
      vi.useRealTimers()
    }
  })

  it('waits for a recording-level request to settle before polling again', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const pending = deferred<number>()
    vi.mocked(getRecordingLevel).mockImplementation(() => pending.promise)
    seedPreviewStatus({ recording: true, recordingInProcess: true, phase: 'Recording' })
    try {
      render(<App />)
      await waitFor(() => expect(getRecordingLevel).toHaveBeenCalledOnce())
      await vi.advanceTimersByTimeAsync(300)
      expect(getRecordingLevel).toHaveBeenCalledOnce()
      pending.resolve(0.25)
      await act(async () => pending.promise)
    } finally {
      vi.useRealTimers()
    }
  })

  it('ignores a serial poll result that settles after unmount', async () => {
    const pending = deferred<number>()
    const request = vi.fn(() => pending.promise)
    const onResult = vi.fn()
    const { unmount } = render(
      <SerialPollHarness request={request} onResult={onResult} />,
    )
    await waitFor(() => expect(request).toHaveBeenCalledOnce())

    unmount()
    pending.resolve(42)
    await act(async () => pending.promise)

    expect(onResult).not.toHaveBeenCalled()
  })

  it('pauses status polling while the document is hidden and resumes one chain', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
    try {
      render(<App />)
      await vi.advanceTimersByTimeAsync(1_200)
      expect(getAppStatus).not.toHaveBeenCalled()

      hidden.mockReturnValue(false)
      await vi.advanceTimersByTimeAsync(400)
      expect(getAppStatus).toHaveBeenCalledOnce()
    } finally {
      hidden.mockRestore()
      vi.useRealTimers()
    }
  })

  it('serializes timed and focus-triggered microphone refreshes', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    const pending = deferred<Awaited<ReturnType<typeof getMicrophones>>>()
    vi.mocked(getMicrophones)
      .mockImplementationOnce(() => actual.getMicrophones())
      .mockImplementation(() => pending.promise)
    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      await waitFor(() => expect(getMicrophones).toHaveBeenCalledTimes(2))

      window.dispatchEvent(new Event('focus'))
      await vi.advanceTimersByTimeAsync(9_000)
      expect(getMicrophones).toHaveBeenCalledTimes(2)
      pending.resolve(await actual.getMicrophones())
      await act(async () => pending.promise)
    } finally {
      vi.useRealTimers()
    }
  })

  it('unlistens when SetupChecklist subscription setup resolves after unmount', async () => {
    const pending = deferred<() => void>()
    const unlisten = vi.fn()
    vi.mocked(onSetupEvent).mockImplementationOnce(() => pending.promise)
    render(<App />)
    await waitFor(() => expect(onSetupEvent).toHaveBeenCalledOnce())

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    pending.resolve(unlisten)

    await waitFor(() => expect(unlisten).toHaveBeenCalledOnce())
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

  it('serializes terminal refreshes without delaying a failed setup event', async () => {
    const listener: { current: ((event: SetupEvent) => void) | null } = { current: null }
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      listener.current = handler
      return Promise.resolve(vi.fn())
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await waitFor(() => expect(listener.current).not.toBeNull())
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    const readiness = await actual.getReadiness()
    const first = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    const second = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    vi.mocked(getReadiness).mockReset()
    vi.mocked(getReadiness)
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const handle = listener.current
    if (!handle) throw new Error('setup listener was not registered')

    act(() => {
      handle({ kind: 'finished', operationId: 'first' })
      handle({ kind: 'failed', operationId: 'second', error: 'second setup failed' })
    })
    expect(screen.getByRole('alert')).toHaveTextContent('second setup failed')
    await waitFor(() => expect(getReadiness).toHaveBeenCalledOnce())

    first.resolve(readiness)
    await act(async () => first.promise)
    await waitFor(() => expect(getReadiness).toHaveBeenCalledTimes(2))

    second.resolve(readiness)
    await act(async () => second.promise)
  })

  it('refreshes SetupChecklist only when setup progress becomes terminal', async () => {
    const listener: { current: ((event: SetupEvent) => void) | null } = { current: null }
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      listener.current = handler
      return Promise.resolve(vi.fn())
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await waitFor(() => expect(listener.current).not.toBeNull())
    vi.mocked(getReadiness).mockClear()
    const handle = listener.current
    if (!handle) throw new Error('setup listener was not registered')

    act(() => {
      handle({
        kind: 'progress',
        progress: {
          operationId: 'install-1',
          component: 'whisper-runtime',
          phase: 'downloading',
          receivedBytes: 25,
          totalBytes: 100,
          resumedFromBytes: 0,
        },
      })
    })
    expect(getReadiness).not.toHaveBeenCalled()

    act(() => handle({ kind: 'cancelled', operationId: 'install-1' }))
    await waitFor(() => expect(getReadiness).toHaveBeenCalledOnce())
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
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
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

  it('does not refresh readiness when a guided microphone test settles after unmount', async () => {
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    const readiness = await actual.getReadiness()
    const result = await actual.testInputDevice(null)
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      firstRunComplete: false,
    })
    const test = deferred<Awaited<ReturnType<typeof testInputDevice>>>()
    vi.mocked(testInputDevice).mockImplementation(() => test.promise)
    const { unmount } = render(<App />)
    const testButton = await screen.findByRole('button', { name: 'Test selected' })
    const readinessCalls = vi.mocked(getReadiness).mock.calls.length
    fireEvent.click(testButton)

    unmount()
    test.resolve(result)
    await act(async () => test.promise)

    expect(getReadiness).toHaveBeenCalledTimes(readinessCalls)
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

  it('shows a readiness refresh rejection after a setup action', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'whisper-small'
          ? {
              ...component,
              managed: { kind: 'ready', version: 'small', bytes: 100, root: '/managed/small' },
            }
          : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('readiness refresh failed'))

    fireEvent.click(await screen.findByRole('button', { name: 'Verify' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('readiness refresh failed')
  })

  it('shows the recording entry point and navigation', async () => {
    render(<App />)
    expect(await screen.findByRole('button', { name: 'Start recording' })).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Echo sections' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Echo' })).toBeInTheDocument()
  })

  it('invokes the visible quit action and reports a rejection', async () => {
    vi.mocked(quitApp).mockRejectedValueOnce(new Error('could not quit Echo'))
    render(<App />)

    const action = screen.getByRole('button', { name: 'Quit Echo' })
    expect(action).toBeVisible()
    fireEvent.click(action)

    await waitFor(() => expect(quitApp).toHaveBeenCalledOnce())
    expect(await screen.findByRole('alert')).toHaveTextContent('could not quit Echo')
  })

  it('navigates, toggles recording, and edits the preview dictionary', async () => {
    render(<App />)
    const start = await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(start)
    expect(await screen.findByRole('button', { name: 'Stop and transcribe' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Dictionary' }))
    fireEvent.change(screen.getByLabelText('What Echo hears'), { target: { value: 'ray cast' } })
    fireEvent.change(screen.getByLabelText('What Echo should write'), { target: { value: 'Raycast' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(await screen.findByText('Raycast')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    expect(screen.getByRole('heading', { name: 'History' })).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search transcripts…')).toBeInTheDocument()
  })

  it('reports a rejected dictionary entry without clearing the form or leaving it busy', async () => {
    vi.mocked(addDictionaryEntry).mockRejectedValueOnce(new Error('could not add dictionary entry'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Dictionary' }))
    const spoken = screen.getByLabelText('What Echo hears')
    const written = screen.getByLabelText('What Echo should write')
    fireEvent.change(spoken, { target: { value: 'ray cast' } })
    fireEvent.change(written, { target: { value: 'Raycast' } })

    fireEvent.click(screen.getByRole('button', { name: 'Add' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('could not add dictionary entry')
    expect(addDictionaryEntry).toHaveBeenCalledWith('ray cast', 'Raycast')
    expect(spoken).toHaveValue('ray cast')
    expect(written).toHaveValue('Raycast')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Add' })).toBeEnabled())
  })

  it('writes a settings change and renders the stored value', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' }))
    expect(within(await screen.findByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' })).toHaveAttribute('data-active', 'true')
    expect((await getPreferences()).hud).toEqual({ value: false, effective: false, source: 'file' })
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

  it('uses the snapped recording limit on Home', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      recordingInProcess: false,
      recordingLimitSeconds: 120,
    })
    render(<App />)
    expect(await screen.findByText('0:00 / 2:00')).toBeInTheDocument()
  })

  it('does not invent a limit for a legacy active status', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      recordingInProcess: false,
      recordingLimitSeconds: null,
    })
    render(<App />)

    expect(await screen.findByText('0:00')).toBeInTheDocument()
    expect(screen.queryByText(/0:00 \/ /)).not.toBeInTheDocument()
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
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
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

  it('shows one fixed toggle shortcut with no customization controls', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findAllByText('Super+Alt+Space')).not.toHaveLength(0)
    expect(screen.queryByRole('button', { name: /Record .*shortcut/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Reset .*shortcut/ })).not.toBeInTheDocument()
    expect(screen.queryByText(/Push-to-talk/i)).not.toBeInTheDocument()
  })

  it('keeps shortcut setup available when editable settings fail to load', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('settings unavailable'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findAllByText('Toggle shortcut')).not.toHaveLength(0)
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeEnabled()
    expect(await screen.findByRole('alert')).toHaveTextContent('settings unavailable')
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
    const { listModels } = await import('./tauri')
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
      phase: 'Failed speech engine failed',
      lastError: 'whisper-cli: ggml_init failed',
    })
    render(<App />)
    await screen.findByText('Failed speech engine failed')
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

  it('keeps component paths collapsed until Installed components opens', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Ready to dictate')).toBeInTheDocument()
    const components = requireFixture(
      screen.getByText('Installed components').closest('details'),
      'Installed components disclosure',
    )
    expect(components).not.toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).not.toBeVisible()
    fireEvent.click(screen.getByText('Installed components'))
    expect(components).toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).toBeVisible()
  })

  it('runs recommended setup and refreshes terminal state', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: false, downloadBytes: 498_000_000 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: /Set up Small multilingual/ }))
    await waitFor(async () => {
      expect((await getReadiness()).plans.find((plan) => plan.id === 'recommended')?.satisfied).toBe(true)
    })
  })

  it('can activate an already available recommended setup', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: true, downloadBytes: 0 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Use Small multilingual' }))
    await waitFor(async () => expect((await getReadiness()).speechReady).toBe(true))
  })

  it('does not start setup while a non-cancellable operation is active', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      activeOperation: 'verify-whisper',
      activeCancellable: false,
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByRole('heading', { name: 'Settings' })
    expect(screen.queryByRole('button', { name: /recommended setup/i })).not.toBeInTheDocument()
  })

  it('offers Use for an already available alternative without duplicating Recommended', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      plans: readiness.plans.map((plan) =>
        plan.id === 'parakeet' ? { ...plan, satisfied: true } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced speech options'))
    const advanced = requireFixture(
      screen.getByText('Advanced speech options').closest('details'),
      'Advanced speech options disclosure',
    )
    const parakeetRow = requireFixture(
      within(advanced).getByText('Parakeet').closest<HTMLElement>('.setting-row'),
      'Parakeet settings row',
    )
    expect(within(parakeetRow).getByRole('button', { name: 'Use' })).toBeEnabled()
    expect(within(advanced).queryByText('Whisper small')).not.toBeInTheDocument()
  })

  it('shows repair and managed-only removal actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'silero-vad'
          ? { ...component, managed: { kind: 'needs-repair', reason: 'wrong size', resumableBytes: 0 } }
          : component.id === 'whisper-small'
            ? { ...component, managed: { kind: 'ready', version: 'small', bytes: 100, root: '/managed/small' } }
            : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByRole('button', { name: 'Repair' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Remove ·/ })).toBeInTheDocument()
  })

  it('shows unsupported guidance without managed mutation actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      managedSupported: false,
      speechReady: false,
      unsupportedReason: 'Managed setup is available on Linux x86_64.',
      components: readiness.components.map((component) => ({
        ...component,
        managed: { kind: 'unsupported', reason: 'Linux x86_64 only' },
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByText('Managed setup is available on Linux x86_64.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Set up Small multilingual/ })).not.toBeInTheDocument()
  })

  it('renders component progress, resume, and low-space admission truthfully', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended'
          ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 900 bytes free; 400 bytes are available' }
          : plan.id === 'parakeet'
            ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 1200 bytes free; 400 bytes are available' }
            : plan,
      ),
      components: readiness.components.map((component) =>
        component.id === 'whisper-small'
          ? {
              ...component,
              managed: { kind: 'absent', resumableBytes: 42 },
              activity: {
                operationId: 'op-1',
                component: component.id,
                phase: 'downloading',
                receivedBytes: 50,
                totalBytes: 100,
                resumedFromBytes: 42,
              },
            }
          : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('progressbar', { name: 'Small multilingual downloading' })).toHaveAttribute('aria-valuenow', '50')
    expect(screen.getByRole('button', { name: /Resume/ })).toBeInTheDocument()
    expect(screen.getByText('Recommended: Needs 900 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByText('Parakeet: Needs 1200 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Set up Small multilingual/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Use Parakeet instead' })).toBeDisabled()
  })

  it('shows microphone then speech as guided Home steps', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      speechReady: false,
      hasSuccessfulDictation: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: false, downloadBytes: 100 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(await screen.findByText('1 · Choose and test a microphone')).toBeInTheDocument()
    expect(screen.getByText('2 · Set up local speech')).toBeInTheDocument()
    expect(screen.getByText('First dictation complete')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: /USB Microphone.*Focusrite/ }))
    await waitFor(() => {
      expect(screen.queryByText('1 · Choose and test a microphone')).not.toBeInTheDocument()
    })
  })

  it('shows usage stats derived from history', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const strip = await screen.findByLabelText('Usage')
    expect(within(strip).getByText('words dictated')).toBeInTheDocument()
    // The three fixture rows carry 8 + 9 + 2 words.
    expect(within(strip).getByText('19')).toBeInTheDocument()
    expect(within(strip).getByText('sessions this week')).toBeInTheDocument()
    expect(within(strip).getByText('day streak')).toBeInTheDocument()
  })

  it('verifies only an explicitly attributed shortcut activation', async () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    // Mic and engine are ready in the fixture; only the shortcut remains open.
    const checklist = await screen.findByLabelText('Finish setup')
    expect(within(checklist).getByText('Shortcut bound')).toBeInTheDocument()
    expect(within(checklist).queryByRole('button', { name: 'I bound it' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    // This activation lands after the last app poll but before testing starts.
    // The test's fresh backend baseline must consume it rather than verify it.
    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:stale-before-click') })
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
    await new Promise((resolve) => window.setTimeout(resolve, 450))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    // An unrelated GUI, tray, or CLI recording phase cannot satisfy the check.
    seedPreviewStatus({ phase: 'Recording', recording: true, recordingInProcess: false })
    await new Promise((resolve) => window.setTimeout(resolve, 30))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('toggle-command:1') })
    await new Promise((resolve) => window.setTimeout(resolve, 150))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:changed', 'Alt+F8') })
    await new Promise((resolve) => window.setTimeout(resolve, 150))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:1') })
    await waitFor(() => expect(localStorage.getItem('echo-shortcut-verified-at')).not.toBeNull())
    expect(stopRecording).toHaveBeenCalledWith('native-toggle:1')
    expect(localStorage.getItem('echo-shortcut-verified-identity')).toBe('portal:Super+Alt+Space')
    expect(await screen.findByText(/Verified/)).toBeInTheDocument()

    seedPreviewStatus({ phase: 'Idle', recording: false, recordingInProcess: false })
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    // Everything is green, so the checklist has done its job and is gone.
    await waitFor(() => expect(screen.queryByLabelText('Finish setup')).not.toBeInTheDocument())
  })

  it('leaves the shortcut unverified when no keypress arrives', async () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    // shouldAdvanceTime keeps the app's status poll and waitFor working
    // while the 10 s listener window is jumped over.
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      seedPreviewStatus({ phase: 'Recording', recording: true, recordingInProcess: true })
      await vi.advanceTimersByTimeAsync(10_100)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
      expect(stopRecording).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('reports a scheduled shortcut poll failure and cleans up an attributed activation', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const pollError = new Error('scheduled shortcut status failed')
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockRejectedValueOnce(pollError)
    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      const activation = shortcutActivation('poll-failure-cleanup')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))

      await act(async () => {
        await vi.advanceTimersByTimeAsync(101)
        await Promise.resolve()
        await Promise.resolve()
      })

      await vi.waitFor(() => {
        expect(getShortcutStatus).toHaveBeenCalledTimes(3)
        expect(stopRecording).toHaveBeenCalledOnce()
        expect(stopRecording).toHaveBeenCalledWith(activation)
      })
      expect(await screen.findByRole('alert')).toHaveTextContent('scheduled shortcut status failed')
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
      expect(localStorage.getItem('echo-shortcut-verified-identity')).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Dismiss error' }))
      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
      await vi.advanceTimersByTimeAsync(1_000)
      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not verify a shortcut when stopping the recording fails', async () => {
    vi.mocked(stopRecording).mockRejectedValueOnce(new Error('cannot stop recording'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:stop-failure') })
    await waitFor(() => expect(stopRecording).toHaveBeenCalledOnce())
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
  })

  it('stops only an attributed shortcut-test recording when Settings unmounts', async () => {
    const { unmount } = render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
    const activation = shortcutActivation('unmount')
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      shortcut: activeShortcut(activation),
    })

    unmount()
    await waitFor(() => expect(stopRecording).toHaveBeenCalledWith(activation))
  })

  it('handles an attributed shortcut cleanup rejection during unmount', async () => {
    const cleanupError = new Error('cleanup could not stop recording')
    const unhandledRejections: unknown[] = []
    const observeUnhandledRejection = (reason: unknown) => {
      unhandledRejections.push(reason)
    }
    process.on('unhandledRejection', observeUnhandledRejection)
    vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut())
    vi.mocked(stopRecording).mockRejectedValueOnce(cleanupError)

    try {
      const { unmount } = render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      const activation = shortcutActivation('cleanup-rejection')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))

      unmount()
      await waitFor(() => expect(stopRecording).toHaveBeenCalledWith(activation))
      await act(async () => {
        await Promise.resolve()
        await new Promise<void>((resolve) => window.setTimeout(resolve, 0))
        await Promise.resolve()
        await new Promise<void>((resolve) => window.setTimeout(resolve, 0))
      })

      expect(unhandledRejections).toEqual([])
    } finally {
      process.off('unhandledRejection', observeUnhandledRejection)
    }
  })

  it('stops an attributed activation returned by an in-flight poll after unmount', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    const unhandledRejections: unknown[] = []
    const observeUnhandledRejection = (reason: unknown) => {
      unhandledRejections.push(reason)
    }
    process.on('unhandledRejection', observeUnhandledRejection)
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)
    vi.mocked(stopRecording).mockRejectedValueOnce(new Error('late activation stop failed'))

    try {
      const { unmount } = render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))

      const activation = shortcutActivation('late-poll')
      unmount()
      expect(getShortcutStatus).toHaveBeenCalledTimes(2)
      expect(stopRecording).not.toHaveBeenCalled()

      poll.resolve(activeShortcut(activation))
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(stopRecording).toHaveBeenCalledOnce()
      expect(stopRecording).toHaveBeenCalledWith(activation)
      expect(unhandledRejections).toEqual([])
    } finally {
      process.off('unhandledRejection', observeUnhandledRejection)
      vi.useRealTimers()
    }
  })

  it('rechecks for an attributed activation after an in-flight unmount snapshot', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)

    try {
      const { unmount } = render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      const activation = shortcutActivation('after-snapshot')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))
      unmount()

      poll.resolve(activeShortcut())
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).toHaveBeenCalledOnce()
      expect(stopRecording).toHaveBeenCalledWith(activation)
    } finally {
      vi.useRealTimers()
    }
  })

  it('rechecks for an attributed activation after an in-flight timeout snapshot', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)

    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      const activation = shortcutActivation('after-timeout-snapshot')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))
      await vi.advanceTimersByTimeAsync(9_900)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()

      poll.resolve(activeShortcut())
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).toHaveBeenCalledOnce()
      expect(stopRecording).toHaveBeenCalledWith(activation)
    } finally {
      vi.useRealTimers()
    }
  })

  it('rechecks for an attributed activation after an in-flight unmount rejection', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)

    try {
      const { unmount } = render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      const activation = shortcutActivation('after-unmount-rejection')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))
      unmount()

      poll.reject(new Error('poll failed after unmount'))
      await act(async () => {
        await poll.promise.catch(() => undefined)
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).toHaveBeenCalledOnce()
      expect(stopRecording).toHaveBeenCalledWith(activation)
    } finally {
      vi.useRealTimers()
    }
  })

  it('rechecks for an attributed activation after an in-flight timeout rejection', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)

    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      const activation = shortcutActivation('after-timeout-rejection')
      vi.mocked(getShortcutStatus).mockResolvedValueOnce(activeShortcut(activation))
      await vi.advanceTimersByTimeAsync(9_900)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()

      poll.reject(new Error('poll failed after timeout'))
      await act(async () => {
        await poll.promise.catch(() => undefined)
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).toHaveBeenCalledOnce()
      expect(stopRecording).toHaveBeenCalledWith(activation)
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not stop a late poll activation that starts after verification times out', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    let activation = ''
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)
      .mockImplementationOnce(() =>
        Promise.resolve(activeShortcut(activation)),
      )

    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      await vi.advanceTimersByTimeAsync(9_900)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()

      activation = shortcutActivation('legitimate-after-timeout', wallClockNow() + 1)
      poll.resolve(activeShortcut(activation))
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not stop a late poll activation that starts after Settings unmounts', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    let activation = ''
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)
      .mockImplementationOnce(() => Promise.resolve(activeShortcut(activation)))

    try {
      const { unmount } = render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      unmount()

      activation = shortcutActivation('legitimate-after-unmount', wallClockNow() + 1)
      poll.resolve(activeShortcut(activation))
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not stop a malformed activation returned after verification times out', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const poll = deferred<ShortcutStatus>()
    vi.mocked(getShortcutStatus)
      .mockResolvedValueOnce(activeShortcut())
      .mockImplementationOnce(() => poll.promise)
      .mockResolvedValueOnce(activeShortcut())

    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

      const activationAt = wallClockNow()
      const seconds = Math.floor(activationAt / 1_000)
      const nanoseconds = Math.floor((activationAt - seconds * 1_000) * 1_000_000)
      const malformedActivation = `native-toggle:${seconds}:${nanoseconds}`

      await vi.advanceTimersByTimeAsync(100)
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      await vi.advanceTimersByTimeAsync(9_900)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()

      poll.resolve(activeShortcut(malformedActivation))
      await act(async () => {
        await poll.promise
        await Promise.resolve()
        await vi.advanceTimersByTimeAsync(0)
      })

      expect(getShortcutStatus).toHaveBeenCalledTimes(3)
      expect(stopRecording).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('lists the microphone step when the mic is not ready', async () => {
    seedPreviewStatus({ microphoneReady: false })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const checklist = await screen.findByLabelText('Finish setup')
    expect(within(checklist).getByText('Microphone ready')).toBeInTheDocument()
    expect(within(checklist).getByText('Speech engine and model installed')).toBeInTheDocument()
  })

  it('parks the level bars for out-of-process sessions and drives them live in-process', async () => {
    seedPreviewStatus({ recording: true, recordingInProcess: false, phase: 'Recording' })
    const { rerender } = render(<App />)
    const parked = await screen.findByText('Listening…')
    expect(parked).toBeInTheDocument()
    const bars = document.querySelector('.level-bars')
    expect(bars).toHaveAttribute('data-live', 'false')

    seedPreviewStatus({ recording: true, recordingInProcess: true, phase: 'Recording' })
    rerender(<App />)
    await waitFor(() => {
      expect(document.querySelector('.level-bars')).toHaveAttribute('data-live', 'true')
    })
    // Live bars get inline heights from the meter.
    await waitFor(() => {
      const heights = [...document.querySelectorAll('.level-bar')].map((bar) => {
        if (!(bar instanceof HTMLElement)) throw new Error('level bar is not an HTML element')
        return bar.style.height
      })
      expect(heights.some((height) => height && height !== '15%')).toBe(true)
    })
  })

  it('renders shortcut setup when settings, models, and microphones fail to load', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('settings unavailable'))
    vi.mocked(listModels).mockRejectedValueOnce(new Error('models unavailable'))
    vi.mocked(getMicrophones).mockRejectedValueOnce(new Error('microphones unavailable'))
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state: 'missing',
          detail: 'GNOME has no Echo custom shortcut yet.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('button', { name: 'Set up GNOME shortcut' })).toBeInTheDocument()
    expect(screen.getAllByText('Super+Alt+Space')).not.toHaveLength(0)
  })


  it.each([
    ['missing', 'Set up GNOME shortcut'],
    ['stale', 'Repair GNOME shortcut'],
  ] as const)('requires an explicit action for a %s GNOME shortcut', async (state, action) => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state,
          detail: state === 'missing' ? 'GNOME has no Echo custom shortcut yet.' : 'The Echo command is old.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const repair = await screen.findByRole('button', { name: action })
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(screen.queryByText('Shortcut verified')).not.toBeInTheDocument()

    fireEvent.click(repair)
    await waitFor(() => expect(repairLegacyShortcut).toHaveBeenCalledOnce())
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    expect(localStorage.getItem('echo-shortcut-verified-identity')).toBeNull()
    expect(await screen.findByText('GNOME shortcut ready')).toBeInTheDocument()
  })

  it('does not offer legacy repair for other native backend failures', async () => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    seedPreviewStatus({
      shortcut: {
        kind: 'failed',
        desired: 'Super+Alt+Space',
        detail: 'portal host registry handshake failed',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(screen.getByText('setup required in Settings')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByText('Shortcut unavailable')

    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('retries an operational shortcut failure explicitly', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'failed',
        desired: 'Super+Alt+Space',
        detail: 'portal shortcut listener stopped',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Retry shortcut' }))
    await waitFor(() => expect(retryShortcut).toHaveBeenCalledOnce())
  })

  it('does not reuse verification from a different shortcut identity', async () => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')
    seedPreviewStatus({ shortcut: activeShortcut(null, 'Ctrl+Alt+Space') })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findAllByText('Ctrl+Alt+Space')).not.toHaveLength(0)

    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(screen.queryByText('Shortcut verified')).not.toBeInTheDocument()
  })

  it('shows stored verification when the shortcut identity resolves after Settings mounts', async () => {
    const nowSeconds = 2_000_000_000
    const dateNow = vi.spyOn(Date, 'now').mockReturnValue(nowSeconds * 1000)
    const status = deferred<AppStatus>()
    vi.mocked(getAppStatus).mockImplementationOnce(() => status.promise)
    localStorage.setItem('echo-shortcut-verified-at', String(nowSeconds))
    localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')

    try {
      render(<App />)
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      await waitFor(() => expect(getAppStatus).toHaveBeenCalledOnce())
      expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()

      status.resolve(richPreviewStatus())
      await act(async () => status.promise)

      await waitFor(() => expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeEnabled())
      expect(screen.getByText(/^Verified /)).toBeInTheDocument()
    } finally {
      dateNow.mockRestore()
    }
  })

  it.each([
    ['malformed', 'not-a-timestamp', false],
    ['future', '2000000001', false],
    ['stale', '1997407999', false],
    ['current valid', '2000000000', true],
  ] as const)('shares %s shortcut verification validity across Home and Settings', async (
    _case,
    rawAt,
    expectedVerified,
  ) => {
    const dateNow = vi.spyOn(Date, 'now').mockReturnValue(2_000_000_000 * 1000)
    try {
      const currentReadiness = await getReadiness()
      seedPreviewReadiness({ ...currentReadiness, firstRunComplete: false })
      localStorage.setItem('echo-shortcut-verified-at', rawAt)
      localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')

      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      const checklist = await screen.findByLabelText('Finish setup')
      if (expectedVerified) {
        expect(within(checklist).getByText('Shortcut verified')).toBeInTheDocument()
        expect(within(checklist).queryByText('Shortcut bound')).not.toBeInTheDocument()
      } else {
        expect(within(checklist).getByText('Shortcut bound')).toBeInTheDocument()
        expect(within(checklist).queryByText('Shortcut verified')).not.toBeInTheDocument()
      }

      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      await screen.findByRole('button', { name: 'Test shortcut' })
      if (expectedVerified) {
        expect(screen.getByText(/^Verified /)).toBeInTheDocument()
      } else {
        expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
      }
    } finally {
      dateNow.mockRestore()
    }
  })

  it('refuses to overwrite a conflicting GNOME shortcut', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state: 'conflicting',
          detail: 'Terminal already uses <Super><Alt>space; change it in GNOME Settings first.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Resolve the GNOME conflict')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('shows ready GNOME setup without offering a write', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-ready',
        desired: 'Super+Alt+Space',
        effective: 'Super+Alt+Space',
        detail: 'GNOME owns this Echo shortcut and its command is current.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
        activation: null,
        verificationIdentity: 'gnome:<Super><Alt>space:/usr/bin/echo-desktop rec --toggle',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('GNOME shortcut ready')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeEnabled()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('gives unsupported Wayland compositors a truthful manual command', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'manual',
        desired: 'Super+Alt+Space',
        detail: 'This Wayland compositor has no GlobalShortcuts portal.',
        command: '/usr/bin/echo-desktop rec --toggle',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(await screen.findByText('Bind it in your desktop settings.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Manual shortcut setup')).toBeInTheDocument()
    expect(screen.getByText('/usr/bin/echo-desktop rec --toggle')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('warns when a stale install shadows the running binary', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    expect(warning).toHaveTextContent('/home/user/.local/bin/echo-desktop')
    expect(warning).toHaveTextContent('rm -f /home/user/.local/bin/echo-desktop')
  })

  it('shows no stale-install warning when PATH is clean', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await screen.findByLabelText('Finish setup')
    expect(screen.queryByText(/shadows this one/)).not.toBeInTheDocument()
  })

  it('removes stale copies in one click and the warning resolves', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    expect(warning).toHaveTextContent('/home/user/.local/bin/echo-desktop')
    // The manual command stays visible as secondary text.
    expect(warning).toHaveTextContent('rm -f /home/user/.local/bin/echo-desktop')

    fireEvent.click(within(warning).getByRole('button', { name: 'Remove old copies' }))
    expect(vi.mocked(removeStaleInstalls)).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
  })

  it('surfaces a removal failure and keeps the warning', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    seedPreviewRemoveStaleError('permission denied')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    fireEvent.click(within(warning).getByRole('button', { name: 'Remove old copies' }))
    expect(await screen.findByText('permission denied')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toBeInTheDocument()
  })

  it('groups history by day', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    // The fixture rows are all from one fixed day; the header follows the
    // Today / Yesterday / date rule relative to when the test runs.
    const day = new Date(1787310400 * 1000)
    day.setHours(0, 0, 0, 0)
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000)
    const expected =
      diffDays === 0
        ? 'Today'
        : diffDays === 1
          ? 'Yesterday'
          : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(day)
    const headers = await screen.findAllByRole('heading', { level: 3 })
    expect(headers.map((header) => header.textContent)).toContain(expected)
  })

  it('labels every history deletion with its transcript text', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))

    for (const text of [
      'This is a test. This is a test.',
      'Open the project settings and update the release notes.',
      'Claude Code.',
    ]) {
      expect(await screen.findByRole('button', { name: `Delete transcript: ${text}` }))
        .toBeInTheDocument()
    }
  })

  it('reports a rejected transcript copy without showing a copied state', async () => {
    vi.mocked(copyText).mockRejectedValueOnce(new Error('clipboard unavailable'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    const text = 'Open the project settings and update the release notes.'
    const row = requireFixture(
      (await screen.findByText(text)).closest('article'),
      'history row for the copied transcript',
    )
    const copyAction = within(row).getByRole('button', { name: 'Copy transcript' })

    fireEvent.click(copyAction)

    expect(await screen.findByRole('alert')).toHaveTextContent('clipboard unavailable')
    expect(copyText).toHaveBeenCalledWith(text)
    expect(within(row).getByRole('button', { name: 'Copy transcript' })).toBe(copyAction)
    expect(within(row).queryByRole('button', { name: 'Copied transcript' })).not.toBeInTheDocument()
  })

  it('keeps a transcript when its deletion confirmation is canceled', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    const text = 'This is a test. This is a test.'

    fireEvent.click(await screen.findByRole('button', { name: `Delete transcript: ${text}` }))

    expect(window.confirm).toHaveBeenCalledOnce()
    expect(deleteHistoryItem).not.toHaveBeenCalled()
    expect(screen.getByText(text)).toBeInTheDocument()
    expect(screen.getByText('Open the project settings and update the release notes.')).toBeInTheDocument()
    expect(screen.getByText('Claude Code.')).toBeInTheDocument()
  })

  it('reports a failed transcript deletion and keeps history unchanged', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    vi.mocked(deleteHistoryItem).mockRejectedValueOnce(new Error('could not delete transcript'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))

    fireEvent.click(await screen.findByRole('button', {
      name: 'Delete transcript: This is a test. This is a test.',
    }))

    expect(await screen.findByRole('alert')).toHaveTextContent('could not delete transcript')
    expect(screen.getByText('This is a test. This is a test.')).toBeInTheDocument()
    expect(screen.getByText('Open the project settings and update the release notes.')).toBeInTheDocument()
    expect(screen.getByText('Claude Code.')).toBeInTheDocument()
  })

  it('deletes one confirmed transcript and updates Home history state', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    const removed = 'This is a test. This is a test.'
    const kept = [
      'Open the project settings and update the release notes.',
      'Claude Code.',
    ]

    fireEvent.click(await screen.findByRole('button', { name: `Delete transcript: ${removed}` }))

    await waitFor(() => expect(deleteHistoryItem).toHaveBeenCalledOnce())
    expect(deleteHistoryItem).toHaveBeenCalledWith('1787310400-11')
    expect(window.confirm).toHaveBeenCalledOnce()
    await waitFor(() => expect(screen.queryByText(removed)).not.toBeInTheDocument())
    kept.forEach((text) => expect(screen.getByText(text)).toBeInTheDocument())

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    expect(await screen.findByText('2 saved transcripts')).toBeInTheDocument()
    const recent = requireFixture(
      screen.getByText('Recent').closest('section'),
      'Recent history section',
    )
    expect(within(recent).queryByText(removed)).not.toBeInTheDocument()
    kept.forEach((text) => expect(within(recent).getByText(text)).toBeInTheDocument())
  })

  it('clears confirmed history and updates Home history state', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))

    fireEvent.click(await screen.findByRole('button', { name: 'Clear all history' }))

    await waitFor(() => expect(clearHistory).toHaveBeenCalledOnce())
    expect(window.confirm).toHaveBeenCalledOnce()
    expect(await screen.findByText('No transcripts yet')).toBeInTheDocument()
    expect(screen.queryByText('This is a test. This is a test.')).not.toBeInTheDocument()
    expect(screen.queryByText('Open the project settings and update the release notes.')).not.toBeInTheDocument()
    expect(screen.queryByText('Claude Code.')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    expect(await screen.findByText('0 saved transcripts')).toBeInTheDocument()
    expect(screen.getByText('No history yet.')).toBeInTheDocument()
    expect(screen.queryByLabelText('Usage')).not.toBeInTheDocument()
  })

  it('reports a failed clear and preserves History and Home history state', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    vi.mocked(clearHistory).mockRejectedValueOnce(new Error('could not clear history'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    const rows = [
      'This is a test. This is a test.',
      'Open the project settings and update the release notes.',
      'Claude Code.',
    ]

    fireEvent.click(await screen.findByRole('button', { name: 'Clear all history' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('could not clear history')
    expect(clearHistory).toHaveBeenCalledOnce()
    expect(window.confirm).toHaveBeenCalledOnce()
    rows.forEach((text) => expect(screen.getByText(text)).toBeInTheDocument())
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Clear all history' })).toBeEnabled()
    })

    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    expect(await screen.findByText('3 saved transcripts')).toBeInTheDocument()
    const recent = requireFixture(
      screen.getByText('Recent').closest('section'),
      'Recent history section',
    )
    rows.forEach((text) => expect(within(recent).getByText(text)).toBeInTheDocument())
  })
})
