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
  await act(async () => render(<HomeView status={status} history={[]} recordingSeconds={0} recordingRequestPending={false} onToggleRecording={onToggleRecording} onOpenSettings={vi.fn()} />))
  expect(screen.getByRole('alert')).toHaveTextContent(status.lastError)
  expect(screen.queryByText('Ready when you are')).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: 'Try recording again' }))
  expect(onToggleRecording).toHaveBeenCalledTimes(1)
})

it('removes the old failure when a new recording starts even if lastError is retained', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: 'Old failure.' }
  const props = { history: [], recordingSeconds: 0, recordingRequestPending: false, onToggleRecording: vi.fn(), onOpenSettings: vi.fn() }
  const rendered = await act(async () => render(<HomeView {...props} status={status} />))
  expect(screen.getByRole('alert')).toHaveTextContent('Old failure.')
  await act(async () => rendered.rerender(<HomeView {...props} status={{ ...status, phase: 'Recording', recordingInProcess: false }} />))
  expect(screen.queryByText('Old failure.')).not.toBeInTheDocument()
  expect(screen.getByRole('heading', { name: 'Listening…' })).toBeInTheDocument()
})

it('provides a failure fallback and blocks retry while the request is pending', async () => {
  const status = { ...api.richPreviewStatus(), phase: 'Failed' as const, lastError: null }
  await act(async () => render(<HomeView status={status} history={[]} recordingSeconds={0} recordingRequestPending onToggleRecording={vi.fn()} onOpenSettings={vi.fn()} />))
  expect(screen.getByRole('alert')).toHaveTextContent('Echo could not finish this recording.')
  expect(screen.getByRole('button', { name: 'Try recording again' })).toBeDisabled()
})
