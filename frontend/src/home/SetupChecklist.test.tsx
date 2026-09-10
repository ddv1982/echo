import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type { SetupEvent } from '../generated/ipc'
import { configureDesktopApi, getMicrophones, getReadiness, onSetupEvent, setMicrophone, testInputDevice } from '../tauri'
import { deferred, resetDesktopApiMocks } from '../test/desktopApiHarness'
import { SetupChecklist } from './SetupChecklist'

const preview = createPreviewDesktopApi()

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  const { createDesktopApiMocks } = await import('../test/desktopApiHarness')
  return { ...actual, ...createDesktopApiMocks(actual) }
})

beforeEach(async () => {
  vi.restoreAllMocks()
  configureDesktopApi(preview)
  preview.resetPreviewSettings()
  const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
  resetDesktopApiMocks(await import('../tauri'), actual)
  preview.seedPreviewReadiness({ ...(await getReadiness()), microphoneReady: false, speechReady: true, firstRunComplete: false })
})

it.each(['failed result', 'rejected request'])('refreshes first-run inputs after a %s', async (failure) => {
  const initial = await getMicrophones()
  render(<SetupChecklist status={preview.richPreviewStatus()} onOpenSettings={vi.fn()} />)
  const testButton = await screen.findByRole('button', { name: 'Test selected' })
  preview.seedPreviewMicrophones({ ...initial, devices: [], systemDefault: null, systemDefaultIsProxy: false, selection: { kind: 'system-default', active: null } })
  if (failure === 'failed result') {
    vi.mocked(testInputDevice).mockResolvedValueOnce({ kind: 'failed', category: 'disconnected', device: null, message: 'Microphone disconnected' })
  } else {
    vi.mocked(testInputDevice).mockRejectedValueOnce(new Error('Microphone disconnected'))
  }

  fireEvent.click(testButton)

  expect(await screen.findByText('No microphone input is available. Connect a microphone and refresh.')).toBeVisible()
  expect(screen.queryByRole('button', { name: 'Test selected' })).not.toBeInTheDocument()
  expect(screen.getAllByRole('radio')).toHaveLength(1)
  expect(screen.getByRole('radio', { name: /Follow system default/ })).toBeEnabled()
})

it('ignores an old test result but releases busy state after selecting another input', async () => {
  const initial = await getMicrophones()
  vi.mocked(getReadiness).mockResolvedValue(await getReadiness())
  const next = initial.devices.find((device) => !device.isDefault && device.tier === 'primary')
  if (!next) throw new Error('Missing selectable microphone fixture')
  const pending = deferred<Awaited<ReturnType<typeof testInputDevice>>>()
  vi.mocked(testInputDevice).mockImplementationOnce(() => pending.promise)
  render(<SetupChecklist status={preview.richPreviewStatus()} onOpenSettings={vi.fn()} />)
  const testButton = await screen.findByRole('button', { name: 'Test selected' })
  fireEvent.click(testButton)
  expect(testButton).toBeDisabled()
  const choice = screen.getByRole('radio', { name: (name) => name.startsWith(next.label) })
  fireEvent.click(choice)
  await waitFor(() => expect(choice).toBeChecked())
  const reads = vi.mocked(getMicrophones).mock.calls.length

  await act(async () => {
    pending.resolve({ kind: 'failed', category: 'disconnected', device: null, message: 'Old input failure' })
    await pending.promise
  })

  await waitFor(() => expect(testButton).toBeEnabled())
  expect(screen.queryByText('Old input failure')).not.toBeInTheDocument()
  expect(vi.mocked(getMicrophones).mock.calls.length).toBe(reads)
})

it('does not replace in-progress setup with a stale readiness fetch', async () => {
  let setupEvent: ((event: SetupEvent) => void) | null = null
  vi.mocked(onSetupEvent).mockImplementation((handler) => {
    setupEvent = handler
    return Promise.resolve(vi.fn())
  })
  const idle = await getReadiness()
  preview.seedPreviewReadiness({
    ...idle,
    microphoneReady: true,
    speechReady: false,
    firstRunComplete: false,
    activeOperation: null,
    activeCancellable: false,
  })
  render(<SetupChecklist status={preview.richPreviewStatus()} onOpenSettings={vi.fn()} />)
  expect(await screen.findByText('Speech setup needed')).toBeInTheDocument()

  const stale = deferred<typeof idle>()
  const readinessCalls = vi.mocked(getReadiness).mock.calls.length
  vi.mocked(getReadiness).mockImplementationOnce(() => stale.promise)
  fireEvent.click(screen.getByRole('button', { name: 'Use Small multilingual' }))
  await waitFor(() => expect(vi.mocked(getReadiness).mock.calls.length).toBeGreaterThan(readinessCalls))
  act(() => {
    setupEvent?.({
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
  expect(screen.getByText('Setting up speech')).toBeInTheDocument()

  await act(async () => {
    stale.resolve({
      ...idle,
      microphoneReady: true,
      speechReady: false,
      firstRunComplete: false,
      activeOperation: null,
      activeCancellable: false,
      components: idle.components.map((component) => ({ ...component, activity: null })),
    })
    await stale.promise
  })

  expect(screen.getByText('Setting up speech')).toBeInTheDocument()
})

it('invalidates an older readiness refresh as soon as microphone selection starts', async () => {
  const initial = await getMicrophones()
  const readiness = await getReadiness()
  const next = initial.devices.find((device) => !device.isDefault && device.tier === 'primary')
  if (!next) throw new Error('Missing selectable microphone fixture')
  const stale = deferred<typeof readiness>()
  const selection = deferred<typeof initial>()
  render(<SetupChecklist status={preview.richPreviewStatus()} onOpenSettings={vi.fn()} />)
  await screen.findByRole('button', { name: 'Test selected' })
  vi.mocked(getReadiness).mockReturnValueOnce(stale.promise)
  vi.mocked(setMicrophone).mockReturnValueOnce(selection.promise)
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
  fireEvent.click(screen.getByRole('radio', { name: (name) => name.startsWith(next.label) }))
  await act(async () => stale.resolve({ ...readiness, microphoneReady: true }))
  expect(screen.getByRole('button', { name: 'Test selected' })).toBeInTheDocument()
  await act(async () => selection.resolve({ ...initial, revision: initial.revision + 1, selection: { kind: 'selected', device: next } }))
})
