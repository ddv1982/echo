import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPreviewDesktopApi } from './api/previewDesktopApi'
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
  startCapture,
  stopCapture,
  cancelTranscription,
} = createPreviewDesktopApi()

describe('settings preview wrappers', () => {
  it('rejects stale stop and cancellation requests without returning another session', async () => {
    const started = await startCapture()
    await expect(stopCapture(`${started.sessionId}-stale`)).rejects.toThrow('session changed')
    await stopCapture(String(started.sessionId))
    await expect(cancelTranscription(`${started.sessionId}-stale`)).rejects.toThrow('session changed')
  })
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
    const reread = await getSettings()
    expect(reread.revision).toBeGreaterThan(written.revision)
    expect(reread.preferences).toEqual(written.preferences)
    expect(reread.transcription).toEqual(written.transcription)
    expect(reread.readiness).toEqual(written.readiness)
  })

  it('snapshots the effective limit when preview recording starts', async () => {
    vi.useFakeTimers()
    try {
      await startCapture()
      expect((await getAppStatus()).recordingLimitSeconds).toBe(600)

      await setSettings({ kind: 'recordSeconds', value: 120 })
      expect((await getAppStatus()).recordingLimitSeconds).toBe(600)
      const shortcut = (await getAppStatus()).shortcut
      if (shortcut.kind !== 'active') throw new Error('active preview shortcut')
      seedPreviewStatus({
        shortcut: { ...shortcut, activation: 'native-toggle:preview-test' },
      })
      expect(await stopRecording('native-toggle:preview-test')).toBe(true)

      await vi.advanceTimersByTimeAsync(900)
      await startCapture()
      expect((await getAppStatus()).recordingLimitSeconds).toBe(120)
    } finally {
      vi.useRealTimers()
    }
  })

  it('stops preview recording at the snapped deadline', async () => {
    vi.useFakeTimers()
    try {
      await setSettings({ kind: 'recordSeconds', value: 1 })
      await startCapture()
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

  it('delegates microphone operations directly to the configured adapter', async () => {
    const preview = createPreviewDesktopApi()
    const initial = await preview.getMicrophones()
    const set = preview.setMicrophone.bind(preview)
    const select = vi.spyOn(preview, 'setMicrophone')
      .mockImplementation((id) => set(id))
    const read = vi.spyOn(preview, 'getMicrophones')
    configureDesktopApi(preview)

    const firstChoice = initial.devices[0]
    const finalChoice = initial.devices[1]
    if (!firstChoice || !finalChoice) throw new Error('preview needs two microphones')
    await Promise.all([
      setConfiguredMicrophone(firstChoice.id),
      setConfiguredMicrophone(finalChoice.id),
    ])
    await getConfiguredMicrophones()

    expect(select).toHaveBeenCalledTimes(2)
    expect(select).toHaveBeenNthCalledWith(1, firstChoice.id)
    expect(select).toHaveBeenNthCalledWith(2, finalChoice.id)
    expect(read).toHaveBeenCalledOnce()
    expect((await getConfiguredMicrophones()).selection).toMatchObject({
      kind: 'selected',
      device: { id: finalChoice.id },
    })
  })

  it('continues microphone operations after a rejected selection', async () => {
    const preview = createPreviewDesktopApi()
    const initial = await preview.getMicrophones()
    const set = preview.setMicrophone.bind(preview)
    const select = vi.spyOn(preview, 'setMicrophone')
      .mockRejectedValueOnce(new Error('device disconnected'))
      .mockImplementation((id) => set(id))
    configureDesktopApi(preview)

    const firstChoice = initial.devices[0]
    const finalChoice = initial.devices[1]
    if (!firstChoice || !finalChoice) throw new Error('preview needs two microphones')
    const rejected = setConfiguredMicrophone(firstChoice.id)
    const finalRequest = setConfiguredMicrophone(finalChoice.id)

    await expect(rejected).rejects.toThrow('device disconnected')
    await finalRequest
    expect(select).toHaveBeenCalledTimes(2)
  })

  it('uses the current adapter for each microphone call', async () => {
    const firstAdapter = createPreviewDesktopApi()
    const secondAdapter = createPreviewDesktopApi()
    const firstSet = vi.spyOn(firstAdapter, 'setMicrophone')
    const secondRead = vi.spyOn(secondAdapter, 'getMicrophones')

    configureDesktopApi(firstAdapter)
    await setConfiguredMicrophone(null)
    configureDesktopApi(secondAdapter)
    await getConfiguredMicrophones()
    expect(firstSet).toHaveBeenCalledOnce()
    expect(secondRead).toHaveBeenCalledOnce()
  })
})
