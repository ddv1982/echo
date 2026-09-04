import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../App'
import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import { HistoryView } from '../history/HistoryView'
import { HomeView } from '../home/HomeView'
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
  ComponentId,
  HistoryItem,
  SettingsChange,
  SetupEvent,
  SetupPlanId,
} from '../generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const {
  resetPreviewSettings,
  richPreviewStatus,
  seedPreviewStatus,
} = previewDesktopApi

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

  it('refreshes persisted history when injection ends in failure', async () => {
    seedPreviewStatus({
      phase: 'Failed',
      lastTranscript: 'recoverable transcript',
      lastHistoryId: 'history-failed-injection',
    })

    render(<App />)

    await waitFor(() => expect(getHistory).toHaveBeenCalledTimes(2))
    expect(await screen.findByText('recoverable transcript')).toBeInTheDocument()
    expect(screen.getByText('Most recently transcribed text')).toBeInTheDocument()
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

  it('refreshes History day headings at local midnight without new history props', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 7, 23, 23, 59, 59, 500))
    const history: HistoryItem[] = [{
      id: 'midnight-history',
      text: 'Captured before midnight',
      raw: 'Captured before midnight',
      engine: 'whisper-small',
      startedAt: Math.floor(new Date(2026, 7, 23, 12).getTime() / 1000),
      inferMs: 100,
      injection: 'Typed',
    }]
    try {
      const { unmount } = render(
        <HistoryView
          items={history}
          onDelete={async () => true}
          onClear={async () => 1}
          onError={vi.fn()}
        />,
      )
      expect(screen.getByRole('heading', { name: 'Today' })).toBeInTheDocument()

      await act(async () => {
        await vi.advanceTimersByTimeAsync(500)
      })

      expect(screen.queryByRole('heading', { name: 'Today' })).not.toBeInTheDocument()
      expect(screen.getByRole('heading', { name: 'Yesterday' })).toBeInTheDocument()
      unmount()
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      vi.useRealTimers()
    }
  })

  it('refreshes Home weekly usage at local midnight without new history props', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 7, 23, 23, 59, 59, 500)) // Sunday
    const history: HistoryItem[] = [{
      id: 'midnight-stats',
      text: 'Sunday session',
      raw: 'Sunday session',
      engine: 'whisper-small',
      startedAt: Math.floor(new Date(2026, 7, 23, 12).getTime() / 1000),
      inferMs: 100,
      injection: 'Typed',
    }]
    try {
      const { unmount } = render(
        <HomeView
          status={richPreviewStatus()}
          history={history}
          recordingSeconds={0}
          onToggleRecording={async () => undefined}
          onOpenSettings={vi.fn()}
        />,
      )
      const label = screen.getByText('sessions this week')
      const stat = requireFixture(label.closest<HTMLElement>('.stat'), 'weekly usage stat')
      expect(stat.querySelector('strong')).toHaveTextContent('1')

      await act(async () => {
        await vi.advanceTimersByTimeAsync(500)
      })

      expect(stat.querySelector('strong')).toHaveTextContent('0')
      unmount()
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      vi.useRealTimers()
    }
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

  it('suppresses a transcript copy rejection after its row unmounts', async () => {
    const copy = deferred<void>()
    vi.mocked(copyText).mockImplementation(() => copy.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    const copyActions = await screen.findAllByRole('button', { name: 'Copy transcript' })
    fireEvent.click(requireFixture(copyActions[0], 'first transcript copy action'))
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))

    await act(async () => {
      copy.reject(new Error('late clipboard failure'))
      await copy.promise.catch(() => undefined)
    })

    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('does not schedule copy feedback after its row unmounts', async () => {
    const copy = deferred<void>()
    vi.mocked(copyText).mockImplementation(() => copy.promise)
    const setTimeoutSpy = vi.spyOn(window, 'setTimeout')
    const view = render(<App />)
    try {
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'History' }))
      const copyActions = await screen.findAllByRole('button', { name: 'Copy transcript' })
      fireEvent.click(requireFixture(copyActions[0], 'first transcript copy action'))
      fireEvent.click(screen.getByRole('button', { name: 'Home' }))
      const feedbackTimers = () => setTimeoutSpy.mock.calls.filter(([, delay]) => delay === 1_200).length
      const timersBeforeSettlement = feedbackTimers()

      copy.resolve()
      await act(async () => copy.promise)

      expect(feedbackTimers()).toBe(timersBeforeSettlement)
    } finally {
      view.unmount()
      setTimeoutSpy.mockRestore()
    }
  })

  it('replaces the owned copy-feedback timer on repeated copies', async () => {
    const firstCopy = deferred<void>()
    const secondCopy = deferred<void>()
    vi.mocked(copyText)
      .mockImplementationOnce(() => firstCopy.promise)
      .mockImplementationOnce(() => secondCopy.promise)
    const setTimeoutSpy = vi.spyOn(window, 'setTimeout')
    const clearTimeoutSpy = vi.spyOn(window, 'clearTimeout')
    const view = render(<App />)
    try {
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'History' }))
      const copyActions = await screen.findAllByRole('button', { name: 'Copy transcript' })
      fireEvent.click(requireFixture(copyActions[0], 'first transcript copy action'))
      firstCopy.resolve()
      await act(async () => firstCopy.promise)
      const copied = await screen.findByRole('button', { name: 'Copied transcript' })
      expect(setTimeoutSpy.mock.calls.some(([, delay]) => delay === 1_200)).toBe(true)
      const clearCallsBeforeSecondClick = clearTimeoutSpy.mock.calls.length

      fireEvent.click(copied)

      expect(clearTimeoutSpy).toHaveBeenCalledTimes(clearCallsBeforeSecondClick + 1)
      secondCopy.resolve()
      await act(async () => secondCopy.promise)
      expect(await screen.findByRole('button', { name: 'Copied transcript' })).toBeInTheDocument()
    } finally {
      view.unmount()
      clearTimeoutSpy.mockRestore()
      setTimeoutSpy.mockRestore()
    }
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
