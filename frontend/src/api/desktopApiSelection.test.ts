import { selectDesktopApi } from './selectDesktopApi'

describe('desktop adapter selection', () => {
  const tauri = { name: 'tauri' }
  const preview = { name: 'preview' }

  it('uses real transport in a Tauri development window', () => {
    expect(selectDesktopApi(true, tauri, preview)).toBe(tauri)
  })

  it('uses preview state in the standalone Vite server', () => {
    expect(selectDesktopApi(false, tauri, preview)).toBe(preview)
  })
})
