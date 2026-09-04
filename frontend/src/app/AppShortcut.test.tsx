import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import process from 'node:process'
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
  onSetupEvent,
  setSettings,
  setMicrophone,
  startSetup,
  stopRecording,
  testInputDevice,
  testMicrophoneFallback,
} from '../tauri'
import type {
  AppStatus,
  ComponentId,
  SettingsChange,
  SetupEvent,
  SetupPlanId,
  ShortcutStatus,
} from '../generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const {
  resetPreviewSettings,
  richPreviewStatus,
  seedPreviewReadiness,
  seedPreviewStatus,
} = previewDesktopApi

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
    seedPreviewStatus({ phase: 'Recording', recordingInProcess: false })
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

    seedPreviewStatus({ phase: 'Idle', recordingInProcess: false })
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
      seedPreviewStatus({ phase: 'Recording', recordingInProcess: true })
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
})
