import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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
  removeStaleInstalls,
  repairLegacyShortcut,
  retryShortcut,
  onSetupEvent,
  setSettings,
  setMicrophone,
  startSetup,
  stopRecording,
  testInputDevice,
  testMicrophoneFallback,
} from './tauri'
import type {
  AppStatus,
  ComponentId,
  SettingsChange,
  SetupEvent,
  SetupPlanId,
} from './generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const {
  resetPreviewSettings,
  richPreviewStatus,
  seedPreviewReadiness,
  seedPreviewRemoveStaleError,
  seedPreviewStatus,
} = previewDesktopApi

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
    seedPreviewStatus({ recordingInProcess: true, phase: 'Recording' })
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

  it.each([
    ['Transcribing', 'Transcribing locally…', 'Transcribing'],
    ['Injecting', 'Inserting your text…', 'Inserting text'],
  ] satisfies Array<[AppStatus['phase'], string, string]>)('keeps %s visible and prevents a second recording', async (phase, title, action) => {
    seedPreviewStatus({ phase })
    render(<App />)
    expect(await screen.findByRole('heading', { name: title })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: action })).toBeDisabled()
    expect(screen.queryByRole('button', { name: 'Start recording' })).not.toBeInTheDocument()
  })

  it('offers a fresh recording after a failed transcription without claiming readiness', async () => {
    seedPreviewStatus({ phase: 'Failed', lastError: 'Speech engine stopped' })
    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Let’s try that again' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Start recording' })).toBeEnabled()
    expect(screen.queryByRole('heading', { name: 'Ready when you are' })).not.toBeInTheDocument()
  })

  it('uses the snapped recording limit on Home', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recordingInProcess: false,
      recordingLimitSeconds: 120,
    })
    render(<App />)
    expect(await screen.findByText('0:00 / 2:00')).toBeInTheDocument()
  })

  it('does not invent a limit for a legacy active status', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recordingInProcess: false,
      recordingLimitSeconds: null,
    })
    render(<App />)

    expect(await screen.findByText('0:00')).toBeInTheDocument()
    expect(screen.queryByText(/0:00 \/ /)).not.toBeInTheDocument()
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

  it('lists the microphone step when the mic is not ready', async () => {
    seedPreviewStatus({ microphoneReady: false })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const checklist = await screen.findByLabelText('Finish setup')
    expect(within(checklist).getByText('Microphone ready')).toBeInTheDocument()
    expect(within(checklist).getByText('Speech engine and model installed')).toBeInTheDocument()
  })

  it('parks the level bars for out-of-process sessions and drives them live in-process', async () => {
    seedPreviewStatus({ recordingInProcess: false, phase: 'Recording' })
    const { rerender } = render(<App />)
    const parked = await screen.findByText('Listening…')
    expect(parked).toBeInTheDocument()
    const bars = document.querySelector('.level-bars')
    expect(bars).toHaveAttribute('data-live', 'false')

    seedPreviewStatus({ recordingInProcess: true, phase: 'Recording' })
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
})
