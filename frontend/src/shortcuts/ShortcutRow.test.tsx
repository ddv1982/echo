import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ShortcutRow } from './ShortcutRow'
import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import { getAppStatus, getShortcutStatus, stopRecording } from '../tauri'
import { resetDesktopApiMocks } from '../test/desktopApiHarness'
import type { AppStatus, ShortcutStatus } from '../generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const { richPreviewStatus } = previewDesktopApi

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  const { createDesktopApiMocks } = await import('../test/desktopApiHarness')
  return { ...actual, ...createDesktopApiMocks(actual) }
})

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

function rowStatus(shortcut: ShortcutStatus): AppStatus {
  return { ...richPreviewStatus(), phase: 'Idle', shortcut }
}

function renderRow(status: AppStatus = rowStatus(activeShortcut())) {
  const onError = vi.fn()
  render(
    <ShortcutRow
      status={status}
      repairing={false}
      onRepair={vi.fn()}
      onRetry={async () => undefined}
      onError={onError}
    />,
  )
  return { onError }
}

describe('ShortcutRow', () => {
  beforeEach(async () => {
    vi.restoreAllMocks()
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const mocks = await import('../tauri')
    resetDesktopApiMocks(mocks, actual)
    vi.mocked(getShortcutStatus).mockResolvedValue(activeShortcut())
    vi.mocked(getAppStatus).mockResolvedValue(rowStatus(activeShortcut()))
    vi.mocked(stopRecording).mockResolvedValue(true)
  })

  it('polls shortcut status after Test shortcut is clicked', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderRow()
      fireEvent.click(screen.getByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledOnce())

      await act(async () => {
        await vi.advanceTimersByTimeAsync(100)
      })

      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledTimes(2))
      expect(stopRecording).not.toHaveBeenCalled()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('stops an attributed activation and marks the shortcut verified', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderRow()
      fireEvent.click(screen.getByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledOnce())
      const activation = shortcutActivation('verified')
      vi.mocked(getShortcutStatus).mockResolvedValue(activeShortcut(activation))

      await act(async () => {
        await vi.advanceTimersByTimeAsync(100)
      })

      await waitFor(() => expect(stopRecording).toHaveBeenCalledWith(activation))
      expect(localStorage.getItem('echo-shortcut-verified-identity')).toBe(
        'portal:Super+Alt+Space',
      )
      expect(await screen.findByText(/Verified/)).toBeInTheDocument()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not verify an activation from another source', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderRow()
      fireEvent.click(screen.getByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      await waitFor(() => expect(getShortcutStatus).toHaveBeenCalledOnce())
      vi.mocked(getShortcutStatus).mockResolvedValue(activeShortcut('toggle-command:1'))

      await act(async () => {
        await vi.advanceTimersByTimeAsync(150)
      })

      expect(stopRecording).not.toHaveBeenCalled()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
      expect(screen.getByText('Listening… press your shortcut')).toBeInTheDocument()
    } finally {
      vi.useRealTimers()
    }
  })
})
