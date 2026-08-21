import { getSettings, setSettings } from './tauri'

describe('settings preview wrappers', () => {
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
})
