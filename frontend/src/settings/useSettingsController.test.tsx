import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type {
  ComponentId,
  MicrophoneSnapshot,
  SettingsChange,
  SettingsSnapshot,
  SetupEvent,
} from '../generated/ipc'
import {
  configureDesktopApi,
  getMicrophones,
  getSettings,
  onSettingsEvent,
  onSetupEvent,
  repairManaged,
  setMicrophone,
  setSettings,
} from '../tauri'
import { useSettingsController } from './useSettingsController'

const previewDesktopApi = createPreviewDesktopApi()

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    getMicrophones: vi.fn(() => actual.getMicrophones()),
    getSettings: vi.fn(() => actual.getSettings()),
    onSettingsEvent: vi.fn((handler: () => void) => actual.onSettingsEvent(handler)),
    onSetupEvent: vi.fn((handler: (event: SetupEvent) => void) => actual.onSetupEvent(handler)),
    repairManaged: vi.fn((component: ComponentId) => actual.repairManaged(component)),
    setMicrophone: vi.fn((id: string | null) => actual.setMicrophone(id)),
    setSettings: vi.fn((change: SettingsChange) => actual.setSettings(change)),
  }
})

function requireFixture<T>(value: T | null | undefined, description: string): T {
  if (value == null) throw new Error(`missing test fixture: ${description}`)
  return value
}

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
    vi.mocked(getMicrophones).mockReset()
    vi.mocked(getMicrophones).mockImplementation(() => actual.getMicrophones())
    vi.mocked(getSettings).mockReset()
    vi.mocked(getSettings).mockImplementation(() => actual.getSettings())
    vi.mocked(onSettingsEvent).mockReset()
    vi.mocked(onSettingsEvent).mockImplementation((handler) => actual.onSettingsEvent(handler))
    vi.mocked(onSetupEvent).mockReset()
    vi.mocked(onSetupEvent).mockImplementation((handler) => actual.onSetupEvent(handler))
    vi.mocked(repairManaged).mockReset()
    vi.mocked(repairManaged).mockImplementation((component) => actual.repairManaged(component))
    vi.mocked(setMicrophone).mockReset()
    vi.mocked(setMicrophone).mockImplementation((id) => actual.setMicrophone(id))
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((change) => actual.setSettings(change))
  })

  it('delegates rapid field changes in call order', async () => {
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
    expect(setSettings).toHaveBeenCalledTimes(2)
    expect(vi.mocked(setSettings).mock.calls.map(([change]) => change)).toEqual([
      { kind: 'language', value: 'en' },
      { kind: 'hud', value: false },
    ])

    await act(async () => releaseFirstWrite.resolve())
    await act(async () => Promise.all([languageWrite, hudWrite]))

    expect(setSettings).toHaveBeenCalledTimes(2)
    expect(result.current.settings?.language.value).toBe('en')
    expect(result.current.settings?.hud.value).toBe(false)
  })

  it('continues settings writes after a rejected write', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    vi.mocked(setSettings)
      .mockRejectedValueOnce(new Error('first write failed'))
      .mockImplementationOnce((change) => actual.setSettings(change))
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.settings).not.toBeNull())

    let failedWrite: Promise<void>
    let successfulWrite: Promise<void>
    act(() => {
      failedWrite = result.current.updateLanguage('en')
      successfulWrite = result.current.updateHud(false)
    })
    await act(async () => Promise.all([failedWrite, successfulWrite]))

    expect(onError).toHaveBeenCalledWith('first write failed')
    expect(vi.mocked(setSettings).mock.calls.map(([change]) => change)).toEqual([
      { kind: 'language', value: 'en' },
      { kind: 'hud', value: false },
    ])
    expect(result.current.settings?.hud.value).toBe(false)
    expect(result.current.settingsWritePending).toBe(false)
  })

  it('does not let a delayed microphone read replace a newer selection', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const initial = await actual.getMicrophones()
    const selectedDevice = requireFixture(initial.devices.find((device) => !device.isDefault), 'selectable microphone')
    const staleRefresh = deferred<MicrophoneSnapshot>()
    const selection = deferred<MicrophoneSnapshot>()
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.microphones).not.toBeNull())
    vi.mocked(getMicrophones).mockImplementationOnce(() => staleRefresh.promise)
    vi.mocked(setMicrophone).mockImplementationOnce(() => selection.promise)

    let refresh: Promise<void>
    act(() => {
      refresh = result.current.refreshMicrophones()
      result.current.selectMicrophone(selectedDevice.id)
    })
    await waitFor(() => expect(setMicrophone).toHaveBeenCalledWith(selectedDevice.id))
    const selected = await actual.setMicrophone(selectedDevice.id)
    selection.resolve(selected)
    await act(async () => selection.promise)
    expect(result.current.microphones?.selection).toMatchObject({
      kind: 'selected',
      device: { id: selectedDevice.id },
    })

    staleRefresh.resolve({
      ...initial,
      revision: selected.revision - 1,
    })
    await act(async () => refresh)
    expect(result.current.microphones?.selection).toMatchObject({
      kind: 'selected',
      device: { id: selectedDevice.id },
    })
  })

  it('does not let the initial settings read replace a newer settings write', async () => {
    const staleSettings = await previewDesktopApi.getSettings()
    const initialRead = deferred<SettingsSnapshot>()
    vi.mocked(getSettings).mockImplementationOnce(() => initialRead.promise)
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledOnce())

    await act(async () => result.current.updateHud(false))
    initialRead.resolve(staleSettings)
    await act(async () => initialRead.promise)

    expect(result.current.settings?.hud.value).toBe(false)
  })

  it('refreshes an open Settings view after a tray settings change', async () => {
    let settingsEvent: (() => void) | null = null
    vi.mocked(onSettingsEvent).mockImplementation((handler) => {
      settingsEvent = handler
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
      expect(settingsEvent).not.toBeNull()
    })
    await previewDesktopApi.setSettings({ kind: 'language', value: 'de' })

    act(() => settingsEvent?.())

    await waitFor(() => expect(result.current.settings?.language.value).toBe('de'))
    expect(onStatusChange).toHaveBeenCalledTimes(2)
    expect(onError).not.toHaveBeenCalled()
  })

  it('establishes the settings listener before its initial read', async () => {
    const registration = deferred<() => void>()
    vi.mocked(onSettingsEvent).mockReturnValue(registration.promise)
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))

    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(getSettings).not.toHaveBeenCalled()
    await previewDesktopApi.setSettings({ kind: 'language', value: 'de' })
    await act(async () => registration.resolve(() => undefined))

    await waitFor(() => expect(result.current.settings?.language.value).toBe('de'))
    expect(getSettings).toHaveBeenCalledOnce()
    expect(onError).not.toHaveBeenCalled()
  })

  it('loads Settings when settings-event registration fails', async () => {
    vi.mocked(onSettingsEvent).mockRejectedValue(new Error('listener unavailable'))
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))

    await waitFor(() => expect(result.current.settings).not.toBeNull())
    expect(onError).toHaveBeenCalledWith('listener unavailable')
    expect(getSettings).toHaveBeenCalledOnce()
  })

  it('keeps the settings-event subscription across rerenders', async () => {
    const unlisten = vi.fn()
    vi.mocked(onSettingsEvent).mockResolvedValue(unlisten)
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result, unmount } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.settings).not.toBeNull())

    await act(async () => result.current.updateHud(false))

    expect(onSettingsEvent).toHaveBeenCalledOnce()
    expect(unlisten).not.toHaveBeenCalled()
    unmount()
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('reloads tray state after a concurrent frontend write fails', async () => {
    let settingsEvent: (() => void) | null = null
    vi.mocked(onSettingsEvent).mockImplementation((handler) => {
      settingsEvent = handler
      return Promise.resolve(vi.fn())
    })
    const trayRead = deferred<SettingsSnapshot>()
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => {
      expect(result.current.settings).not.toBeNull()
      expect(settingsEvent).not.toBeNull()
    })
    await previewDesktopApi.setSettings({ kind: 'language', value: 'de' })
    vi.mocked(getSettings)
      .mockImplementationOnce(() => trayRead.promise)
      .mockImplementationOnce(() => actual.getSettings())
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('write failed'))

    act(() => settingsEvent?.())
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2))
    await act(async () => result.current.updateHud(false))
    trayRead.resolve(await actual.getSettings())
    await act(async () => trayRead.promise)

    await waitFor(() => expect(result.current.settings?.language.value).toBe('de'))
    expect(onError).toHaveBeenCalledWith('write failed')
  })

  it('does not let an older setup read replace a newer tray read', async () => {
    let setupEvent: ((event: SetupEvent) => void) | null = null
    let settingsEvent: (() => void) | null = null
    vi.mocked(onSetupEvent).mockImplementation((handler) => {
      setupEvent = handler
      return Promise.resolve(vi.fn())
    })
    vi.mocked(onSettingsEvent).mockImplementation((handler) => {
      settingsEvent = handler
      return Promise.resolve(vi.fn())
    })
    const staleSettings = await previewDesktopApi.getSettings()
    const setupRead = deferred<SettingsSnapshot>()
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => {
      expect(result.current.settings).not.toBeNull()
      expect(setupEvent).not.toBeNull()
      expect(settingsEvent).not.toBeNull()
    })
    vi.mocked(getSettings)
      .mockImplementationOnce(() => setupRead.promise)
      .mockImplementationOnce(() => actual.getSettings())

    act(() => setupEvent?.({ kind: 'finished', operationId: 'setup' }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2))
    await previewDesktopApi.setSettings({ kind: 'language', value: 'de' })
    act(() => settingsEvent?.())
    await waitFor(() => expect(result.current.settings?.language.value).toBe('de'))
    setupRead.resolve(staleSettings)
    await act(async () => setupRead.promise)

    expect(result.current.settings?.language.value).toBe('de')
    expect(onError).not.toHaveBeenCalled()
  })

  it('delegates readiness refreshes while a backend settings write is queued', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const writeStarted = deferred<void>()
    const releaseWrite = deferred<void>()
    vi.mocked(setSettings).mockImplementationOnce(async (change) => {
      writeStarted.resolve()
      await releaseWrite.promise
      return actual.setSettings(change)
    })
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.settings).not.toBeNull())

    let write: Promise<void>
    act(() => {
      write = result.current.updateHud(false)
    })
    await writeStarted.promise
    await act(async () => {
      result.current.refreshReadiness()
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(getSettings).toHaveBeenCalledTimes(2)

    await act(async () => releaseWrite.resolve())
    await act(async () => write)
    expect(result.current.settings?.hud.value).toBe(false)
  })

  it('delegates GPU repair refreshes while a backend settings write is queued', async () => {
    const actual = await vi.importActual<typeof import('../tauri')>('../tauri')
    const initialSettings = await previewDesktopApi.getSettings()
    const component = initialSettings.readiness.components.find(
      (candidate) => candidate.id === 'whisper-vulkan-runtime',
    )
    if (!component) throw new Error('preview GPU prerequisite is missing')
    const gpuSettings: SettingsSnapshot = {
      ...initialSettings,
      preferences: {
        ...initialSettings.preferences,
        whisperAcceleration: {
          ...initialSettings.preferences.whisperAcceleration,
          effective: 'gpu',
        },
      },
      transcription: {
        ...initialSettings.transcription,
        whisper: { kind: 'applicable', gpu: { kind: 'needs-install', component } },
      },
    }
    vi.mocked(getSettings).mockResolvedValueOnce(gpuSettings)
    vi.mocked(repairManaged).mockResolvedValueOnce('gpu-repair')
    const writeStarted = deferred<void>()
    const releaseWrite = deferred<void>()
    vi.mocked(setSettings).mockImplementationOnce(async (change) => {
      writeStarted.resolve()
      await releaseWrite.promise
      return actual.setSettings(change)
    })
    const onStatusChange = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const { result } = renderHook(() => useSettingsController({
      onStatusChange,
      onError,
    }))
    await waitFor(() => expect(result.current.gpuPrerequisite).not.toBeNull())

    let write: Promise<void>
    act(() => {
      write = result.current.updateHud(false)
    })
    await writeStarted.promise
    await act(async () => {
      result.current.installGpuPrerequisite()
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(repairManaged).toHaveBeenCalledWith('whisper-vulkan-runtime')
    expect(getSettings).toHaveBeenCalledTimes(2)

    await act(async () => releaseWrite.resolve())
    await act(async () => write)
    expect(result.current.settings?.hud.value).toBe(false)
  })

  it('does not let a snapshot without a revision replace a newer settings write', async () => {
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
    const missingRevision = { ...staleSettings }
    Object.defineProperty(missingRevision, 'revision', { value: undefined })
    const staleRefresh = deferred<Awaited<ReturnType<typeof getSettings>>>()
    vi.mocked(getSettings).mockImplementationOnce(() => staleRefresh.promise)

    act(() => setupEvent?.({ kind: 'finished', operationId: 'setup' }))
    await waitFor(() => expect(getSettings).toHaveBeenCalledTimes(2))
    await act(async () => result.current.updateHud(false))

    staleRefresh.resolve(missingRevision)
    await act(async () => staleRefresh.promise)
    await waitFor(() => expect(result.current.settings?.hud.value).toBe(false))
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
