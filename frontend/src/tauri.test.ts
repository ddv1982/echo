import { createPreviewDesktopApi } from './api/previewDesktopApi'

const {
  getAppStatus,
  getSettings,
  resetPreviewSettings,
  seedPreviewStatus,
  setSettings,
  stopRecording,
  toggleRecording,
} = createPreviewDesktopApi()

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
      expect((await getAppStatus()).recording).toBe(true)

      await vi.advanceTimersByTimeAsync(1_001)
      expect((await getAppStatus()).recording).toBe(false)
      expect((await getAppStatus()).phase).toBe('Transcribing')
      await vi.advanceTimersByTimeAsync(900)
      expect((await getAppStatus()).phase).toBe('Idle')
    } finally {
      vi.useRealTimers()
    }
  })
})
