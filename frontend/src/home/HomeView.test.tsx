import { act, fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import { configureDesktopApi } from '../tauri'
import { HomeView } from './HomeView'

const api = createPreviewDesktopApi()
beforeEach(() => configureDesktopApi(api))

it('shows a failed recording reason and allows retry from Home', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: 'Microphone disconnected.' }
  const onToggleRecording = vi.fn(() => Promise.resolve())
  await act(async () => render(<HomeView status={status} history={[]} recordingSeconds={0} cancellationPending={false} onCancelTranscription={vi.fn()} recordingRequestPending={false} onToggleRecording={onToggleRecording} onOpenSettings={vi.fn()} />))
  expect(screen.getByRole('alert')).toHaveTextContent(status.lastError)
  expect(screen.queryByText('Ready when you are')).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: 'Try recording again' }))
  expect(onToggleRecording).toHaveBeenCalledTimes(1)
})

it('removes the old failure when a new recording starts even if lastError is retained', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: 'Old failure.' }
  const props = { cancellationPending: false, onCancelTranscription: vi.fn(), history: [], recordingSeconds: 0, recordingRequestPending: false, onToggleRecording: vi.fn(), onOpenSettings: vi.fn() }
  const rendered = await act(async () => render(<HomeView {...props} status={status} />))
  expect(screen.getByRole('alert')).toHaveTextContent('Old failure.')
  await act(async () => rendered.rerender(<HomeView {...props} status={{ ...status, phase: 'Recording', recordingInProcess: false }} />))
  expect(screen.queryByText('Old failure.')).not.toBeInTheDocument()
  expect(screen.getByRole('heading', { name: 'Listening…' })).toBeInTheDocument()
})

it('provides a failure fallback and blocks retry while the request is pending', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: null }
  await act(async () => render(<HomeView status={status} history={[]} recordingSeconds={0} cancellationPending={false} onCancelTranscription={vi.fn()} recordingRequestPending onToggleRecording={vi.fn()} onOpenSettings={vi.fn()} />))
  expect(screen.getByRole('alert')).toHaveTextContent('Echo could not finish this recording.')
  expect(screen.getByRole('button', { name: 'Try recording again' })).toBeDisabled()
})

it('offers cancellation only for an identified transcription and shows pending feedback', async () => {
  const props = { history: [], recordingSeconds: 0, recordingRequestPending: false, cancellationPending: false, onCancelTranscription: vi.fn(), onToggleRecording: vi.fn(), onOpenSettings: vi.fn() }
  const status = { ...api.richPreviewStatus(), phase: 'Transcribing' as const, recordingSessionId: 'session-a' }
  const view = await act(async () => render(<HomeView {...props} status={status} />))
  fireEvent.click(screen.getByRole('button', { name: 'Cancel transcription' }))
  expect(props.onCancelTranscription).toHaveBeenCalledTimes(1)
  await act(async () => view.rerender(<HomeView {...props} cancellationPending status={status} />))
  expect(screen.getByRole('button', { name: 'Canceling transcription…' })).toBeDisabled()
  await act(async () => view.rerender(<HomeView {...props} status={{ ...status, phase: 'Injecting' }} />))
  expect(screen.queryByRole('button', { name: /Cancel/ })).not.toBeInTheDocument()
  await act(async () => view.rerender(<HomeView {...props} status={{ ...status, recordingSessionId: null }} />))
  expect(screen.queryByRole('button', { name: /Cancel/ })).not.toBeInTheDocument()
})

it('presents acknowledged cancellation separately from recording failure', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: 'Transcription canceled' }
  await act(async () => render(<HomeView status={status} history={[]} recordingSeconds={0} recordingRequestPending={false} cancellationPending={false} onCancelTranscription={vi.fn()} onToggleRecording={vi.fn()} onOpenSettings={vi.fn()} />))
  expect(screen.getByRole('heading', { name: 'Transcription canceled' })).toBeInTheDocument()
  expect(screen.getByRole('status')).toHaveTextContent('before text was inserted')
  expect(screen.queryByRole('alert')).not.toBeInTheDocument()
})
