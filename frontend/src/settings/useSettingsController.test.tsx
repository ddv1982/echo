import { act, renderHook, waitFor } from '@testing-library/react'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type { SetupEvent } from '../generated/ipc'
import {
  configureDesktopApi,
  getReadiness,
  getSettings,
  listLanguages,
  listModels,
  onSetupEvent,
  setSettings,
} from '../tauri'
import { useSettingsController } from './useSettingsController'

const previewDesktopApi = createPreviewDesktopApi()

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    getReadiness: vi.fn(() => actual.getReadiness()),
    getSettings: vi.fn(() => actual.getSettings()),
    listLanguages: vi.fn(() => actual.listLanguages()),
    listModels: vi.fn(() => actual.listModels()),
    onSetupEvent: vi.fn((handler) => actual.onSetupEvent(handler)),
    setSettings: vi.fn((settings) => actual.setSettings(settings)),
  }
})

function deferred<T>() {
  let resolvePromise: ((value: T | PromiseLike<T>) => void) | null = null
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!resolvePromise) throw new Error('deferred promise is not initialized')
      resolvePromise(value)
    },
  }
}

describe('useSettingsController', () => {
  beforeEach(async () => {
    configureDesktopApi(previewDesktopApi)
    previewDesktopApi.resetPreviewSettings()
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    vi.mocked(getReadiness).mockReset()
    vi.mocked(getReadiness).mockImplementation(() => actual.getReadiness())
    vi.mocked(getSettings).mockReset()
    vi.mocked(getSettings).mockImplementation(() => actual.getSettings())
    vi.mocked(listLanguages).mockReset()
    vi.mocked(listLanguages).mockImplementation(() => actual.listLanguages())
    vi.mocked(listModels).mockReset()
    vi.mocked(listModels).mockImplementation(() => actual.listModels())
    vi.mocked(onSetupEvent).mockReset()
    vi.mocked(onSetupEvent).mockImplementation((handler) => actual.onSetupEvent(handler))
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((settings) => actual.setSettings(settings))
  })

  it('applies queued writes in call order against the latest stored settings', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const firstWriteStarted = deferred<void>()
    const releaseFirstWrite = deferred<void>()
    vi.mocked(setSettings).mockImplementationOnce(async (settings) => {
      firstWriteStarted.resolve()
      await releaseFirstWrite.promise
      return actual.setSettings(settings)
    })
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()

    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.settings).not.toBeNull())

    let cleanupWrite: Promise<void>
    let hudWrite: Promise<void>
    act(() => {
      cleanupWrite = result.current.patch('cleanup', 'off')
      hudWrite = result.current.patch('hud', false)
    })
    await firstWriteStarted.promise
    expect(setSettings).toHaveBeenCalledOnce()

    await act(async () => releaseFirstWrite.resolve())
    await act(async () => Promise.all([cleanupWrite, hudWrite]))

    expect(setSettings).toHaveBeenCalledTimes(2)
    const secondWrite = vi.mocked(setSettings).mock.calls[1][0]
    expect(secondWrite.cleanup.value).toBe('off')
    expect(secondWrite.hud.value).toBe(false)
  })

  it('does not let a stale setup refresh replace a newer settings write', async () => {
    let setupEvent: ((event: SetupEvent) => void) | null = null
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      setupEvent = handler
      return Promise.resolve(vi.fn())
    })
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => {
      expect(result.current.settings).not.toBeNull()
      expect(setupEvent).not.toBeNull()
    })
    const staleSettings = result.current.settings
    if (!staleSettings) throw new Error('settings did not load')
    const staleRefresh = deferred<Awaited<ReturnType<typeof getSettings>>>()
    vi.mocked(getSettings).mockImplementationOnce(() => staleRefresh.promise)

    act(() => setupEvent?.({ kind: 'finished', operationId: 'setup' }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2))
    await act(async () => result.current.patch('cleanup', 'off'))

    staleRefresh.resolve(staleSettings)
    await act(async () => staleRefresh.promise)
    await waitFor(() => expect(result.current.settings?.cleanup.value).toBe('off'))
  })
})
