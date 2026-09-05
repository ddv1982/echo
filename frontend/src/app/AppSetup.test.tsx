import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { StrictMode } from 'react'
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
import { deferred } from '../test/desktopApiHarness'
import type {
  ComponentId,
  SettingsChange,
  SetupEvent,
  SetupPlanId,
} from '../generated/ipc'

const previewDesktopApi = createPreviewDesktopApi()
const {
  resetPreviewSettings,
  seedPreviewReadiness,
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

  it('unlistens when SetupChecklist subscription setup resolves after unmount', async () => {
    const pending = deferred<() => void>()
    const unlisten = vi.fn()
    vi.mocked(onSetupEvent).mockImplementationOnce(() => pending.promise)
    render(<App />)
    await waitFor(() => expect(onSetupEvent).toHaveBeenCalledOnce())

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    pending.resolve(unlisten)

    await waitFor(() => expect(unlisten).toHaveBeenCalledOnce())
  })

  it('does not let stale StrictMode SetupChecklist loads replace replayed results', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    const microphones = await actual.getMicrophones()
    const staleReadiness = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    const currentReadiness = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    const staleMicrophones = deferred<Awaited<ReturnType<typeof getMicrophones>>>()
    const currentMicrophones = deferred<Awaited<ReturnType<typeof getMicrophones>>>()
    vi.mocked(getReadiness)
      .mockImplementationOnce(() => staleReadiness.promise)
      .mockImplementationOnce(() => currentReadiness.promise)
    vi.mocked(getMicrophones)
      .mockImplementationOnce(() => staleMicrophones.promise)
      .mockImplementationOnce(() => currentMicrophones.promise)

    render(<StrictMode><App /></StrictMode>)
    await waitFor(() => {
      expect(getReadiness).toHaveBeenCalledTimes(2)
      expect(getMicrophones).toHaveBeenCalledTimes(2)
    })

    await act(async () => {
      currentReadiness.resolve({
        ...readiness,
        microphoneReady: false,
        speechReady: true,
        firstRunComplete: false,
      })
      currentMicrophones.resolve({
        ...microphones,
        enumerationWarning: 'current microphone inventory',
      })
      await Promise.all([currentReadiness.promise, currentMicrophones.promise])
    })
    expect(await screen.findByText(
      'Some microphones could not be listed: current microphone inventory',
    )).toBeVisible()
    const speechItem = requireFixture(
      screen.getByText('Speech engine and model installed').closest('.checklist-item'),
      'speech readiness checklist item',
    )
    expect(speechItem).toHaveAttribute('data-done', 'true')

    await act(async () => {
      staleReadiness.resolve({
        ...readiness,
        microphoneReady: false,
        speechReady: false,
        firstRunComplete: false,
      })
      staleMicrophones.resolve({
        ...microphones,
        enumerationWarning: 'stale microphone inventory',
      })
      await Promise.all([staleReadiness.promise, staleMicrophones.promise])
    })

    expect(speechItem).toHaveAttribute('data-done', 'true')
    expect(screen.getByText(
      'Some microphones could not be listed: current microphone inventory',
    )).toBeVisible()
    expect(screen.queryByText(
      'Some microphones could not be listed: stale microphone inventory',
    )).not.toBeInTheDocument()
  })


  it('serializes terminal refreshes without delaying a failed setup event', async () => {
    const listener: { current: ((event: SetupEvent) => void) | null } = { current: null }
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      listener.current = handler
      return Promise.resolve(vi.fn())
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await waitFor(() => expect(listener.current).not.toBeNull())
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    const first = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    const second = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    vi.mocked(getReadiness).mockReset()
    vi.mocked(getReadiness)
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const handle = listener.current
    if (!handle) throw new Error('setup listener was not registered')

    act(() => {
      handle({ kind: 'finished', operationId: 'first' })
      handle({ kind: 'failed', operationId: 'second', error: 'second setup failed' })
    })
    expect(screen.getByRole('alert')).toHaveTextContent('second setup failed')
    await waitFor(() => expect(getReadiness).toHaveBeenCalledOnce())

    first.resolve(readiness)
    await act(async () => first.promise)
    await waitFor(() => expect(getReadiness).toHaveBeenCalledTimes(2))

    second.resolve(readiness)
    await act(async () => second.promise)
  })

  it('refreshes SetupChecklist only when setup progress becomes terminal', async () => {
    const listener: { current: ((event: SetupEvent) => void) | null } = { current: null }
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      listener.current = handler
      return Promise.resolve(vi.fn())
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await waitFor(() => expect(listener.current).not.toBeNull())
    vi.mocked(getReadiness).mockClear()
    const handle = listener.current
    if (!handle) throw new Error('setup listener was not registered')

    act(() => {
      handle({
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
    expect(getReadiness).not.toHaveBeenCalled()

    act(() => handle({ kind: 'cancelled', operationId: 'install-1' }))
    await waitFor(() => expect(getReadiness).toHaveBeenCalledOnce())
  })


  it('does not report a Settings setup rejection that settles after navigation', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
    })
    const setup = deferred<string>()
    vi.mocked(startSetup).mockImplementation(() => setup.promise)
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const action = await screen.findByRole('button', { name: 'Use Small multilingual' })

    fireEvent.click(action)
    await waitFor(() => expect(startSetup).toHaveBeenCalledWith('recommended'))
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))

    await act(async () => {
      setup.reject(new Error('late Settings setup failure'))
      await setup.promise.catch(() => undefined)
    })

    expect(document.querySelector('.main-content > .error-banner[role="alert"]')).toBeNull()
    expect(screen.queryByText('late Settings setup failure')).not.toBeInTheDocument()
  })


  it('does not refresh readiness when a guided microphone test settles after unmount', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    const result = await actual.testInputDevice(null)
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      firstRunComplete: false,
    })
    const test = deferred<Awaited<ReturnType<typeof testInputDevice>>>()
    vi.mocked(testInputDevice).mockImplementation(() => test.promise)
    const { unmount } = render(<App />)
    const testButton = await screen.findByRole('button', { name: 'Test selected' })
    const readinessCalls = vi.mocked(getReadiness).mock.calls.length
    fireEvent.click(testButton)

    unmount()
    test.resolve(result)
    await act(async () => test.promise)

    expect(getReadiness).toHaveBeenCalledTimes(readinessCalls)
  })

  it('does not continue a guided microphone selection after unmount', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    const microphones = await actual.getMicrophones()
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      firstRunComplete: false,
    })
    const selection = deferred<Awaited<ReturnType<typeof setMicrophone>>>()
    vi.mocked(setMicrophone).mockImplementation(() => selection.promise)
    render(<App />)
    const choices = await screen.findAllByRole('radio', { name: /USB Microphone/ })

    fireEvent.click(requireFixture(choices[0], 'guided microphone choice'))
    await waitFor(() => expect(setMicrophone).toHaveBeenCalledOnce())
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Microphone')
    const readinessCalls = vi.mocked(getReadiness).mock.calls.length

    selection.resolve(microphones)
    await act(async () => selection.promise)

    expect(getReadiness).toHaveBeenCalledTimes(readinessCalls)
  })

  it('settles a rejected guided microphone refresh after unmount', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const readiness = await actual.getReadiness()
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      firstRunComplete: false,
    })
    const { unmount } = render(<App />)
    await screen.findByRole('button', { name: 'Test selected' })
    const microphones = deferred<Awaited<ReturnType<typeof getMicrophones>>>()
    const readinessRefresh = deferred<Awaited<ReturnType<typeof getReadiness>>>()
    vi.mocked(getMicrophones).mockClear()
    vi.mocked(getReadiness).mockClear()
    vi.mocked(getMicrophones).mockImplementation(() => microphones.promise)
    vi.mocked(getReadiness).mockImplementation(() => readinessRefresh.promise)

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => {
      expect(getMicrophones).toHaveBeenCalledOnce()
      expect(getReadiness).toHaveBeenCalledOnce()
    })
    unmount()

    await act(async () => {
      microphones.reject(new Error('late microphone refresh failure'))
      readinessRefresh.resolve(readiness)
      await Promise.allSettled([microphones.promise, readinessRefresh.promise])
    })

    expect(screen.queryByText('late microphone refresh failure')).not.toBeInTheDocument()
  })

  it.each(['resolve', 'reject'] as const)(
    'does not refresh or report when a guided speech action %s after unmount',
    async (outcome) => {
      const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
      const readiness = await actual.getReadiness()
      seedPreviewReadiness({
        ...readiness,
        speechReady: false,
        firstRunComplete: false,
      })
      const setup = deferred<string>()
      vi.mocked(startSetup).mockImplementation(() => setup.promise)
      render(<App />)
      const action = await screen.findByRole('button', { name: 'Use Small multilingual' })

      fireEvent.click(action)
      await waitFor(() => expect(startSetup).toHaveBeenCalledWith('recommended'))
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      await screen.findByLabelText('Microphone')
      const readinessCalls = vi.mocked(getReadiness).mock.calls.length

      await act(async () => {
        if (outcome === 'resolve') setup.resolve('late-operation')
        else setup.reject(new Error('late setup failure'))
        await setup.promise.catch(() => undefined)
      })

      expect(getReadiness).toHaveBeenCalledTimes(readinessCalls)
      expect(screen.queryByText('late setup failure')).not.toBeInTheDocument()
    },
  )

  it('shows a readiness refresh rejection after a setup action', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'whisper-small'
          ? {
              ...component,
              managed: { kind: 'ready', version: 'small', bytes: 100, root: '/managed/small' },
            }
          : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('readiness refresh failed'))

    fireEvent.click(await screen.findByRole('button', { name: 'Verify' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('readiness refresh failed')
  })

  it('keeps component paths collapsed until Installed components opens', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Ready to dictate')).toBeInTheDocument()
    const components = requireFixture(
      screen.getByText('Installed components').closest('details'),
      'Installed components disclosure',
    )
    expect(components).not.toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).not.toBeVisible()
    fireEvent.click(screen.getByText('Installed components'))
    expect(components).toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).toBeVisible()
  })

  it('runs recommended setup and refreshes terminal state', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: false, downloadBytes: 498_000_000 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: /Set up Small multilingual/ }))
    await waitFor(async () => {
      expect((await getReadiness()).plans.find((plan) => plan.id === 'recommended')?.satisfied).toBe(true)
    })
  })

  it('can activate an already available recommended setup', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: true, downloadBytes: 0 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Use Small multilingual' }))
    await waitFor(async () => expect((await getReadiness()).speechReady).toBe(true))
  })

  it('does not start setup while a non-cancellable operation is active', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      activeOperation: 'verify-whisper',
      activeCancellable: false,
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByRole('heading', { name: 'Settings' })
    expect(screen.queryByRole('button', { name: /recommended setup/i })).not.toBeInTheDocument()
  })

  it('offers Use for an already available alternative without duplicating Recommended', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      plans: readiness.plans.map((plan) =>
        plan.id === 'parakeet' ? { ...plan, satisfied: true } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced speech options'))
    const advanced = requireFixture(
      screen.getByText('Advanced speech options').closest('details'),
      'Advanced speech options disclosure',
    )
    const parakeetRow = requireFixture(
      within(advanced).getByText('Parakeet').closest<HTMLElement>('.setting-row'),
      'Parakeet settings row',
    )
    expect(within(parakeetRow).getByRole('button', { name: 'Use' })).toBeEnabled()
    expect(within(advanced).queryByText('Whisper small')).not.toBeInTheDocument()
  })

  it('shows repair and managed-only removal actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'silero-vad'
          ? { ...component, managed: { kind: 'needs-repair', reason: 'wrong size', resumableBytes: 0 } }
          : component.id === 'whisper-small'
            ? { ...component, managed: { kind: 'ready', version: 'small', bytes: 100, root: '/managed/small' } }
            : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByRole('button', { name: 'Repair' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Remove ·/ })).toBeInTheDocument()
  })

  it('shows unsupported guidance without managed mutation actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      managedSupported: false,
      speechReady: false,
      unsupportedReason: 'Managed setup is available on Linux x86_64.',
      components: readiness.components.map((component) => ({
        ...component,
        managed: { kind: 'unsupported', reason: 'Linux x86_64 only' },
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByText('Managed setup is available on Linux x86_64.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Set up Small multilingual/ })).not.toBeInTheDocument()
  })

  it('renders component progress, resume, and low-space admission truthfully', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended'
          ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 900 bytes free; 400 bytes are available' }
          : plan.id === 'parakeet'
            ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 1200 bytes free; 400 bytes are available' }
            : plan,
      ),
      components: readiness.components.map((component) =>
        component.id === 'whisper-small'
          ? {
              ...component,
              managed: { kind: 'absent', resumableBytes: 42 },
              activity: {
                operationId: 'op-1',
                component: component.id,
                phase: 'downloading',
                receivedBytes: 50,
                totalBytes: 100,
                resumedFromBytes: 42,
              },
            }
          : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('progressbar', { name: 'Small multilingual downloading' })).toHaveAttribute('aria-valuenow', '50')
    expect(screen.getByRole('button', { name: /Resume/ })).toBeInTheDocument()
    expect(screen.getByText('Recommended: Needs 900 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByText('Parakeet: Needs 1200 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Set up Small multilingual/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Use Parakeet instead' })).toBeDisabled()
  })
})
