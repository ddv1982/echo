import {
  getAppStatus,
  getSettings,
  resetPreviewSettings,
  seedPreviewStatus,
  setSettings,
  stopRecording,
  toggleRecording,
} from './tauri'

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
    expect((await getSettings()).recordSeconds).toEqual({
      value: null,
      effective: 600,
      source: 'default',
    })
  })

  it('setSettings mutates the preview fixture', async () => {
    const before = await getSettings()
    expect(before.engine).toEqual({ value: null, effective: 'auto', source: 'default' })
    const written = await setSettings({
      ...before,
      engine: { ...before.engine, value: 'fake' },
      hud: { ...before.hud, value: false },
      recordSeconds: { ...before.recordSeconds, value: 12 },
    })
    expect(written.engine).toEqual({ value: 'fake', effective: 'fake', source: 'file' })
    expect(written.hud).toEqual({ value: false, effective: false, source: 'file' })
    expect(written.recordSeconds).toEqual({ value: 12, effective: 12, source: 'file' })
    expect(await getSettings()).toEqual(written)
  })

  it('snapshots the effective limit when preview recording starts', async () => {
    await toggleRecording()
    expect((await getAppStatus()).recordingLimitSeconds).toBe(600)

    const settings = await getSettings()
    await setSettings({
      ...settings,
      recordSeconds: { ...settings.recordSeconds, value: 120 },
    })
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
})
