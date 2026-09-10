import { render, screen } from '@testing-library/react'
import { expect, it } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import { StatusPill } from './chrome'

it('presents cancellation without a failed badge and retains genuine failure styling', () => {
  const status = { ...createPreviewDesktopApi().richPreviewStatus(), phase: 'Failed' as const, lastError: 'Transcription canceled' }
  const view = render(<StatusPill status={status} />)
  expect(screen.getByLabelText('Echo status: Canceled')).toHaveAttribute('data-tone', 'ready')
  expect(screen.queryByText('Failed')).not.toBeInTheDocument()
  view.rerender(<StatusPill status={{ ...status, lastError: 'Microphone disconnected' }} />)
  expect(screen.getByLabelText('Echo status: Failed')).toHaveAttribute('data-tone', 'error')
  view.rerender(<StatusPill status={{ ...status, phase: 'Recording' }} />)
  expect(screen.getByLabelText('Echo status: Recording')).toHaveAttribute('data-tone', 'recording')
})
