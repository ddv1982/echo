import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { createPreviewDesktopApi } from './api/previewDesktopApi'
import { useSerialPoll } from './hooks/useSerialPoll'
import {
  addDictionaryEntry,
  configureDesktopApi,
  getAppStatus,
  getRecordingLevel,
  getMicrophones,
  getReadiness,
  quitApp,
  removeStaleInstalls,
  toggleRecording,
} from './tauri'
import { deferred, resetDesktopApiMocks } from './test/desktopApiHarness'
import type {
  AppStatus,
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
  const { createDesktopApiMocks } = await import('./test/desktopApiHarness')
  return { ...actual, ...createDesktopApiMocks(actual) }
})

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
    const mocks = await import('./tauri')
    resetDesktopApiMocks(mocks, actual)
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

  it.each([
    ['Transcribing', 'Transcribing locally…', 'Whisper · small · VAD on is turning your recording into text.'],
    ['Injecting', 'Inserting transcript…', 'ydotool · Wayland is sending your transcript to the active app.'],
  ] satisfies Array<[AppStatus['phase'], string, string]>)(
    'prevents a second recording toggle while %s',
    async (phase, heading, description) => {
      seedPreviewStatus({ phase, recordingInProcess: false })
      render(<App />)

      const orb = await screen.findByRole('button', { name: 'Processing recording' })
      expect(orb).toBeDisabled()
      expect(screen.getByRole('heading', { name: heading })).toBeInTheDocument()
      expect(screen.getByText(description)).toBeInTheDocument()
      fireEvent.click(orb)
      expect((await previewDesktopApi.getAppStatus()).phase).toBe(phase)
    },
  )

  it.each(['Idle', 'Failed'] as const)(
    'allows recording to start again from %s',
    async (phase) => {
      seedPreviewStatus({
        phase,
        recordingInProcess: false,
      })
      render(<App />)

      const orb = await screen.findByRole('button', { name: 'Start recording' })
      expect(orb).toBeEnabled()
      fireEvent.click(orb)
      fireEvent.click(orb)
      expect(await screen.findByRole('button', { name: 'Stop and transcribe' })).toBeEnabled()
    },
  )

  it('keeps a successful stop pending through stale statuses and polling errors', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const recording = { ...richPreviewStatus(), phase: 'Recording' } satisfies AppStatus
    const transcribing = { ...recording, phase: 'Transcribing', recordingInProcess: false } satisfies AppStatus
    const idle = { ...recording, phase: 'Idle', recordingInProcess: false } satisfies AppStatus
    vi.mocked(toggleRecording).mockResolvedValueOnce(undefined)
    vi.mocked(getAppStatus)
      .mockResolvedValueOnce(recording)
      .mockResolvedValueOnce(recording)
      .mockRejectedValueOnce(new Error('temporary status error'))
      .mockResolvedValueOnce(transcribing)
      .mockResolvedValueOnce(idle)
    try {
      render(<App />)
      const stop = await screen.findByRole('button', { name: 'Stop and transcribe' })
      fireEvent.click(stop)
      await act(async () => {})
      expect(toggleRecording).toHaveBeenCalledOnce()

      const stopping = screen.getByRole('button', { name: 'Stopping recording' })
      expect(stopping).toBeDisabled()
      fireEvent.click(stopping)
      expect(toggleRecording).toHaveBeenCalledOnce()

      await act(async () => vi.advanceTimersByTimeAsync(400))
      expect(screen.getByRole('button', { name: 'Stopping recording' })).toBeDisabled()
      await act(async () => vi.advanceTimersByTimeAsync(400))
      expect(screen.getByRole('button', { name: 'Processing recording' })).toBeDisabled()
      await act(async () => vi.advanceTimersByTimeAsync(400))
      expect(screen.getByRole('button', { name: 'Start recording' })).toBeEnabled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('releases a rejected stop request for retry', async () => {
    const recording = { ...richPreviewStatus(), phase: 'Recording' } satisfies AppStatus
    vi.mocked(getAppStatus).mockResolvedValue(recording)
    vi.mocked(toggleRecording)
      .mockRejectedValueOnce(new Error('stop was rejected'))
      .mockResolvedValueOnce(undefined)
    render(<App />)

    fireEvent.click(await screen.findByRole('button', { name: 'Stop and transcribe' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('stop was rejected')
    fireEvent.click(screen.getByRole('button', { name: 'Stop and transcribe' }))
    await waitFor(() => expect(toggleRecording).toHaveBeenCalledTimes(2))
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
      firstPathHit: '/home/user/.local/bin/echo desktop; keep-me',
      staleInstalls: ['/home/user/.local/bin/echo desktop; keep-me'],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    expect(warning).toHaveTextContent('/home/user/.local/bin/echo desktop; keep-me')
    expect(warning).not.toHaveTextContent('rm -f')
    expect(within(warning).getByRole('button', { name: 'Remove old copies' })).toBeEnabled()
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
    expect(warning).not.toHaveTextContent('rm -f')

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
