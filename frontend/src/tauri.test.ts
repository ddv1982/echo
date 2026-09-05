import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPreviewDesktopApi } from './api/previewDesktopApi'
import type { MicrophoneSnapshot } from './generated/ipc'
import {
  configureDesktopApi,
  getMicrophones as getConfiguredMicrophones,
  setMicrophone as setConfiguredMicrophone,
} from './tauri'

const {
  getAppStatus,
  getSettings,
  resetPreviewSettings,
  seedPreviewStatus,
  setSettings,
  stopRecording,
  toggleRecording,
} = createPreviewDesktopApi()

function deferred<T>() {
  const state: {
    resolve: ((value: T | PromiseLike<T>) => void) | null
    reject: ((reason?: unknown) => void) | null
  } = { resolve: null, reject: null }
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    state.resolve = resolvePromise
    state.reject = rejectPromise
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!state.resolve) throw new Error('deferred promise is not initialized')
      state.resolve(value)
    },
    reject(reason?: unknown) {
      if (!state.reject) throw new Error('deferred promise is not initialized')
      state.reject(reason)
    },
  }
}

describe('settings preview wrappers', () => {
  beforeEach(() => resetPreviewSettings())

  it('mirrors the Rust recording policy in one preview fixture', async () => {
    const status = await getAppStatus()
    expect(status.recordingPolicy).toEqual({
      minimumSeconds: 1,
      defaultSeconds: 600,
      maximumSeconds: 600,
      presetsSeconds: [30, 60, 120, 300, 600],
    })
    expect(status.recordingLimitSeconds).toBe(600)
    expect((await getSettings()).preferences.recordSeconds).toEqual({
      value: null,
      effective: 600,
      source: 'default',
    })
  })

  it('setSettings mutates the preview fixture', async () => {
    const before = await getSettings()
    expect(before.preferences.engine).toEqual({ value: null, effective: 'auto', source: 'default' })
    await setSettings({ kind: 'engine', value: 'fake' })
    await setSettings({ kind: 'hud', value: false })
    const written = await setSettings({ kind: 'recordSeconds', value: 12 })
    expect(written.preferences.engine).toEqual({ value: 'fake', effective: 'fake', source: 'file' })
    expect(written.preferences.hud).toEqual({ value: false, effective: false, source: 'file' })
    expect(written.preferences.recordSeconds).toEqual({ value: 12, effective: 12, source: 'file' })
    expect(await getSettings()).toEqual(written)
  })

  it('snapshots the effective limit when preview recording starts', async () => {
    await toggleRecording()
    expect((await getAppStatus()).recordingLimitSeconds).toBe(600)

    await setSettings({ kind: 'recordSeconds', value: 120 })
    expect((await getAppStatus()).recordingLimitSeconds).toBe(600)
    const shortcut = (await getAppStatus()).shortcut
    if (shortcut.kind !== 'active') throw new Error('active preview shortcut')
    seedPreviewStatus({
      shortcut: { ...shortcut, activation: 'native-toggle:preview-test' },
    })
    expect(await stopRecording('native-toggle:preview-test')).toBe(true)

    await toggleRecording()
    expect((await getAppStatus()).recordingLimitSeconds).toBe(120)
  })

  it('stops preview recording at the snapped deadline', async () => {
    vi.useFakeTimers()
    try {
      await setSettings({ kind: 'recordSeconds', value: 1 })
      await toggleRecording()
      expect((await getAppStatus()).phase).toBe('Recording')

      await vi.advanceTimersByTimeAsync(1_001)
      expect((await getAppStatus()).phase).not.toBe('Recording')
      expect((await getAppStatus()).phase).toBe('Transcribing')
      await vi.advanceTimersByTimeAsync(900)
      expect((await getAppStatus()).phase).toBe('Idle')
    } finally {
      vi.useRealTimers()
    }
  })

  it('serializes microphone selections so the final choice wins', async () => {
    const preview = createPreviewDesktopApi()
    const initial = await preview.getMicrophones()
    const first = deferred<MicrophoneSnapshot>()
    const set = preview.setMicrophone.bind(preview)
    const select = vi.spyOn(preview, 'setMicrophone')
      .mockImplementationOnce(() => first.promise)
      .mockImplementation((id) => set(id))
    configureDesktopApi(preview)

    const firstChoice = initial.devices[0]
    const finalChoice = initial.devices[1]
    if (!firstChoice || !finalChoice) throw new Error('preview needs two microphones')
    const firstRequest = setConfiguredMicrophone(firstChoice.id)
    const finalRequest = setConfiguredMicrophone(finalChoice.id)
    await Promise.resolve()
    expect(select).toHaveBeenCalledOnce()

    first.resolve(initial)
    await firstRequest
    await finalRequest
    expect(select).toHaveBeenNthCalledWith(1, firstChoice.id)
    expect(select).toHaveBeenNthCalledWith(2, finalChoice.id)
    expect((await getConfiguredMicrophones()).selection).toMatchObject({
      kind: 'selected',
      device: { id: finalChoice.id },
    })
  })

  it('continues microphone operations after a rejected selection and keeps reads ordered', async () => {
    const preview = createPreviewDesktopApi()
    const initial = await preview.getMicrophones()
    const reject = deferred<MicrophoneSnapshot>()
    const set = preview.setMicrophone.bind(preview)
    const select = vi.spyOn(preview, 'setMicrophone')
      .mockImplementationOnce(() => reject.promise)
      .mockImplementation((id) => set(id))
    const read = vi.spyOn(preview, 'getMicrophones')
    configureDesktopApi(preview)

    const firstChoice = initial.devices[0]
    const finalChoice = initial.devices[1]
    if (!firstChoice || !finalChoice) throw new Error('preview needs two microphones')
    const rejected = setConfiguredMicrophone(firstChoice.id)
    const finalRequest = setConfiguredMicrophone(finalChoice.id)
    const readRequest = getConfiguredMicrophones()
    await Promise.resolve()
    expect(select).toHaveBeenCalledOnce()
    expect(read).not.toHaveBeenCalled()

    reject.reject(new Error('device disconnected'))
    await expect(rejected).rejects.toThrow('device disconnected')
    await finalRequest
    await readRequest
    expect(select).toHaveBeenCalledTimes(2)
    expect(read).toHaveBeenCalledOnce()
  })

  it('keeps queued microphone work with the adapter that accepted it', async () => {
    const firstAdapter = createPreviewDesktopApi()
    const secondAdapter = createPreviewDesktopApi()
    const initial = await firstAdapter.getMicrophones()
    const pending = deferred<MicrophoneSnapshot>()
    const firstSet = vi.spyOn(firstAdapter, 'setMicrophone').mockImplementation(() => pending.promise)
    const secondRead = vi.spyOn(secondAdapter, 'getMicrophones')
    const choice = initial.devices[0]
    if (!choice) throw new Error('preview needs a microphone')

    configureDesktopApi(firstAdapter)
    const firstRequest = setConfiguredMicrophone(choice.id)
    configureDesktopApi(secondAdapter)
    await getConfiguredMicrophones()
    expect(firstSet).toHaveBeenCalledOnce()
    expect(secondRead).toHaveBeenCalledOnce()

    pending.resolve(initial)
    await firstRequest
  })
})
