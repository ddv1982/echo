import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type { RecordingSnapshot } from '../generated/ipc'
import { configureDesktopApi } from '../tauri'
import { createDesktopApiMocks, deferred } from '../test/desktopApiHarness'
import { useAppController } from './useAppController'

const preview = createPreviewDesktopApi()
const api = createDesktopApiMocks(preview)
const transcribing = () => ({ ...preview.richPreviewStatus(), phase: 'Transcribing' as const, recordingSessionId: 'session-a', recordingRevision: 2 })
const acknowledgement: RecordingSnapshot = { phase: 'Transcribing', sessionId: 'session-a', revision: 3, captureStopRequested: false }

beforeEach(() => {
  vi.mocked(api.getAppStatus).mockReset().mockResolvedValue(transcribing())
  vi.mocked(api.cancelTranscription).mockReset()
  configureDesktopApi(api)
})

it('retains pending cancellation through acknowledgement and rejects duplicate requests', async () => {
  const pending = deferred<RecordingSnapshot>()
  vi.mocked(api.cancelTranscription).mockReturnValue(pending.promise)
  const { result } = renderHook(useAppController)
  await waitFor(() => expect(result.current.status.phase).toBe('Transcribing'))
  let request: Promise<void>
  let duplicate: Promise<void>
  await act(async () => {
    request = result.current.cancelTranscription()
    duplicate = result.current.cancelTranscription()
    await duplicate
  })
  expect(api.cancelTranscription).toHaveBeenCalledExactlyOnceWith('session-a')
  expect(result.current.cancellationPending).toBe(true)
  vi.mocked(api.getAppStatus).mockResolvedValue({ ...transcribing(), recordingRevision: 3 })
  await act(async () => { pending.resolve(acknowledgement); await request })
  await act(async () => result.current.cancelTranscription())
  expect(api.cancelTranscription).toHaveBeenCalledTimes(1)
  expect(result.current.cancellationPending).toBe(true)
  vi.mocked(api.getAppStatus).mockResolvedValue({ ...transcribing(), phase: 'Failed', recordingRevision: 4, lastError: 'Transcription canceled' })
  await act(async () => result.current.refreshStatus())
  expect(result.current.status.lastError).toBe('Transcription canceled')
  expect(result.current.cancellationPending).toBe(false)
})

it.each(['acknowledgement', 'rejection'] as const)('ignores an old cancellation %s after a replacement session', async (outcome) => {
  const pending = deferred<RecordingSnapshot>()
  vi.mocked(api.cancelTranscription).mockReturnValue(pending.promise)
  const { result } = renderHook(useAppController)
  await waitFor(() => expect(result.current.status.phase).toBe('Transcribing'))
  let request: Promise<void>
  act(() => { request = result.current.cancelTranscription() })
  const replacement = { ...transcribing(), phase: 'Recording' as const, recordingSessionId: 'session-b', recordingRevision: 1 }
  vi.mocked(api.getAppStatus).mockResolvedValue(replacement)
  await act(async () => result.current.refreshStatus())
  await act(async () => {
    if (outcome === 'acknowledgement') pending.resolve(acknowledgement)
    else pending.reject(new Error('Old cancellation rejected'))
    await request
  })
  expect(result.current.status).toEqual(replacement)
  expect(result.current.error).toBeNull()
  expect(result.current.cancellationPending).toBe(false)
})

it('reports a cancellation rejected at insertion and does not cancel while injecting', async () => {
  vi.mocked(api.cancelTranscription).mockRejectedValue(new Error('Insertion already started.'))
  const { result } = renderHook(useAppController)
  await waitFor(() => expect(result.current.status.phase).toBe('Transcribing'))
  await act(async () => result.current.cancelTranscription())
  expect(result.current.error).toBe('Insertion already started.')
  expect(result.current.cancellationPending).toBe(false)
  vi.mocked(api.getAppStatus).mockResolvedValue({ ...transcribing(), phase: 'Injecting', recordingRevision: 4 })
  await act(async () => result.current.refreshStatus())
  await act(async () => result.current.cancelTranscription())
  expect(api.cancelTranscription).toHaveBeenCalledTimes(1)
})

it('does not cancel a legacy transcription without a session identity', async () => {
  vi.mocked(api.getAppStatus).mockResolvedValue({ ...transcribing(), recordingSessionId: null })
  const { result } = renderHook(useAppController)
  await waitFor(() => expect(result.current.status.phase).toBe('Transcribing'))
  await act(async () => result.current.cancelTranscription())
  expect(api.cancelTranscription).not.toHaveBeenCalled()
})
