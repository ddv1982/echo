import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type { SettingsChange, SetupEvent } from '../generated/ipc'
import {
  configureDesktopApi,
  getSettings,
  onSetupEvent,
  setSettings,
} from '../tauri'
import { useSettingsController } from './useSettingsController'

const previewDesktopApi = createPreviewDesktopApi()

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    getSettings: vi.fn(() => actual.getSettings()),
    onSetupEvent: vi.fn((handler: (event: SetupEvent) => void) => actual.onSetupEvent(handler)),
    setSettings: vi.fn((change: SettingsChange) => actual.setSettings(change)),
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
    vi.mocked(getSettings).mockReset()
    vi.mocked(getSettings).mockImplementation(() => actual.getSettings())
    vi.mocked(onSetupEvent).mockReset()
    vi.mocked(onSetupEvent).mockImplementation((handler) => actual.onSetupEvent(handler))
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((change) => actual.setSettings(change))
  })

  it('applies queued field changes in call order', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const firstWriteStarted = deferred<void>()
    const releaseFirstWrite = deferred<void>()
    vi.mocked(setSettings).mockImplementationOnce(async (change) => {
      firstWriteStarted.resolve()
      await releaseFirstWrite.promise
      return actual.setSettings(change)
    })
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()

    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.settings).not.toBeNull())

    let languageWrite: Promise<void>
    let hudWrite: Promise<void>
    act(() => {
      languageWrite = result.current.updateLanguage('en')
      hudWrite = result.current.updateHud(false)
    })
    await firstWriteStarted.promise
    expect(setSettings).toHaveBeenCalledOnce()

    await act(async () => releaseFirstWrite.resolve())
    await act(async () => Promise.all([languageWrite, hudWrite]))

    expect(setSettings).toHaveBeenCalledTimes(2)
    expect(vi.mocked(setSettings).mock.calls.map(([change]) => change)).toEqual([
      { kind: 'language', value: 'en' },
      { kind: 'hud', value: false },
    ])
    expect(result.current.settings?.language.value).toBe('en')
    expect(result.current.settings?.hud.value).toBe(false)
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
    const staleSettings = await previewDesktopApi.getSettings()
    const staleRefresh = deferred<Awaited<ReturnType<typeof getSettings>>>()
    vi.mocked(getSettings).mockImplementationOnce(() => staleRefresh.promise)

    act(() => setupEvent?.({ kind: 'finished', operationId: 'setup' }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2))
    await act(async () => result.current.updateHud(false))

    staleRefresh.resolve(staleSettings)
    await act(async () => staleRefresh.promise)
    await waitFor(() => expect(result.current.settings?.hud.value).toBe(false))
  })

  it('waits for setup status propagation before starting a later terminal refresh', async () => {
    let setupEvent: ((event: SetupEvent) => void) | null = null
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      setupEvent = handler
      return Promise.resolve(vi.fn())
    })
    const statusSettlement = deferred<void>()
    const onStatusChange = vi.fn(() => statusSettlement.promise)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => {
      expect(result.current.settings).not.toBeNull()
      expect(setupEvent).not.toBeNull()
    })

    act(() => setupEvent?.({ kind: 'finished', operationId: 'first' }))
    await waitFor(() => expect(onStatusChange).toHaveBeenCalledOnce())

    await act(async () => {
      setupEvent?.({ kind: 'finished', operationId: 'second' })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(getSettings).toHaveBeenCalledTimes(2)

    await act(async () => statusSettlement.resolve())
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(3))
  })

  it('reports a rejected setup status propagation through the settings error path', async () => {
    let setupEvent: ((event: SetupEvent) => void) | null = null
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      setupEvent = handler
      return Promise.resolve(vi.fn())
    })
    const onStatusChange = vi.fn().mockRejectedValue(new Error('status refresh failed'))
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => {
      expect(result.current.settings).not.toBeNull()
      expect(setupEvent).not.toBeNull()
    })

    act(() => setupEvent?.({ kind: 'finished', operationId: 'setup' }))

    await waitFor(() => expect(onError).toHaveBeenCalledWith('status refresh failed'))
  })
})
