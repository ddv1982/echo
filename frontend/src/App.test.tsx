import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import App from './App'
import {
  getSettings,
  removeStaleInstalls,
  repairLegacyShortcut,
  resetPreviewSettings,
  seedPreviewLanguages,
  seedPreviewMicTestError,
  seedPreviewRemoveStaleError,
  seedPreviewSettings,
  seedPreviewStatus,
  setSettings,
} from './tauri'

vi.mock('./tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./tauri')>()
  return {
    ...actual,
    setSettings: vi.fn((settings) => actual.setSettings(settings)),
    removeStaleInstalls: vi.fn(() => actual.removeStaleInstalls()),
    repairLegacyShortcut: vi.fn(() => actual.repairLegacyShortcut()),
  }
})

describe('Echo desktop shell', () => {
  beforeEach(async () => {
    resetPreviewSettings()
    localStorage.removeItem('echo-shortcut-bound')
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((settings) => actual.setSettings(settings))
    vi.mocked(repairLegacyShortcut).mockReset()
    vi.mocked(repairLegacyShortcut).mockImplementation(() => actual.repairLegacyShortcut())
  })

  it('shows the recording entry point and navigation', async () => {
    render(<App />)
    expect(await screen.findByRole('button', { name: 'Start recording' })).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: 'Echo sections' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Echo' })).toBeInTheDocument()
  })

  it('navigates, toggles recording, and edits the preview dictionary', async () => {
    render(<App />)
    const start = await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(start)
    expect(await screen.findByRole('button', { name: 'Stop and transcribe' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Dictionary' }))
    fireEvent.change(screen.getByLabelText('What Whisper hears'), { target: { value: 'ray cast' } })
    fireEvent.change(screen.getByLabelText('What Echo should write'), { target: { value: 'Raycast' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(await screen.findByText('Raycast')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    expect(screen.getByRole('heading', { name: 'History' })).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Search transcripts…')).toBeInTheDocument()
  })

  it('writes a settings change and renders the stored value', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced'))
    fireEvent.click(within(await screen.findByRole('group', { name: 'Cleanup' })).getByRole('button', { name: 'Off' }))
    expect(within(await screen.findByRole('group', { name: 'Cleanup' })).getByRole('button', { name: 'Off' })).toHaveAttribute('data-active', 'true')
    expect((await getSettings()).cleanup).toEqual({ value: 'off', effective: 'off', source: 'file' })

    fireEvent.click(screen.getByRole('button', { name: 'Record Push-to-talk shortcut' }))
    fireEvent.keyDown(window, { key: 'Shift', code: 'ShiftRight' })
    fireEvent.keyUp(window, { key: 'Shift', code: 'ShiftRight' })
    await waitFor(async () => {
      expect((await getSettings()).holdKey).toEqual({
        value: 'RightShift',
        effective: 'RightShift',
        source: 'file',
      })
    })
  })

  it('disables an env-backed field and names the variable', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      cleanup: { value: null, effective: 'off', source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const off = within(await screen.findByRole('group', { name: 'Cleanup' })).getByRole('button', { name: 'Off' })
    expect(off).toBeDisabled()
    expect(off).toHaveAttribute('data-active', 'true')
    expect(screen.getByText('ECHO_CLEANUP')).toBeInTheDocument()
    fireEvent.click(within(await screen.findByRole('group', { name: 'Cleanup' })).getByRole('button', { name: 'Rules' }))
    expect((await getSettings()).cleanup.effective).toBe('off')
  })

  it('persists two rapid settings writes', async () => {
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    let releaseFirst = () => {}
    let signalFirstStarted = () => {}
    const firstWriteStarted = new Promise<void>((resolve) => {
      signalFirstStarted = resolve
    })
    const firstWriteGate = new Promise<void>((resolve) => {
      releaseFirst = resolve
    })
    let writes = 0
    vi.mocked(setSettings).mockImplementation(async (settings) => {
      writes += 1
      if (writes === 1) {
        signalFirstStarted()
        await firstWriteGate
      }
      return actual.setSettings(settings)
    })

    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced'))
    fireEvent.click(within(await screen.findByRole('group', { name: 'Cleanup' })).getByRole('button', { name: 'Off' }))
    await firstWriteStarted
    fireEvent.click(screen.getByRole('button', { name: 'Record Toggle shortcut' }))
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft' })
    fireEvent.keyDown(window, { key: 't', code: 'KeyT' })
    releaseFirst()

    await waitFor(async () => {
      const stored = await getSettings()
      expect(stored.cleanup).toEqual({ value: 'off', effective: 'off', source: 'file' })
      expect(stored.toggleShortcut).toEqual({
        value: 'Ctrl+T',
        effective: 'Ctrl+T',
        source: 'file',
      })
    })
  })

  it('shows the error banner when a settings save fails', async () => {
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('could not write settings'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced'))
    const cleanup = await screen.findByRole('group', { name: 'Cleanup' })
    fireEvent.click(within(cleanup).getByRole('button', { name: 'Off' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('could not write settings')
    expect(within(cleanup).getByRole('button', { name: 'Off' })).toHaveAttribute('data-active', 'false')
  })

  it('captures, normalizes, and persists a toggle chord without recording', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record Toggle shortcut' }))
    fireEvent.keyDown(window, { key: 'p', code: 'KeyP' })
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
    expect(screen.getByText('Press keys…')).toBeInTheDocument()
    fireEvent.keyDown(window, { key: 'Alt', code: 'AltLeft' })
    fireEvent.keyDown(window, { key: 'Meta', code: 'MetaLeft' })
    expect(screen.getByText('Super+Alt+…')).toBeInTheDocument()
    fireEvent.keyDown(window, { key: 'p', code: 'KeyP' })

    await waitFor(async () => {
      expect((await getSettings()).toggleShortcut).toEqual({
        value: 'Super+Alt+P',
        effective: 'Super+Alt+P',
        source: 'file',
      })
    })
    expect(screen.getByLabelText('Echo status: Idle')).toBeInTheDocument()
    expect(vi.mocked(setSettings)).toHaveBeenCalledTimes(1)
  })

  it('keeps toggle capture open after a lone modifier is released', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record Toggle shortcut' }))

    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft' })
    fireEvent.keyUp(window, { key: 'Control', code: 'ControlLeft' })
    expect(screen.getByText('Press keys…')).toBeInTheDocument()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()

    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft' })
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK' })
    await waitFor(async () => {
      expect((await getSettings()).toggleShortcut.effective).toBe('Ctrl+K')
    })
  })

  it('cancels shortcut capture with Escape and switches to the other recorder', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record Toggle shortcut' }))
    expect(screen.getAllByText('Press keys…')).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: 'Record Push-to-talk shortcut' }))
    expect(screen.getAllByText('Press keys…')).toHaveLength(1)
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    expect(screen.queryByText('Press keys…')).not.toBeInTheDocument()
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
    expect((await getSettings()).holdKey.effective).toBe('RightCtrl')
  })

  it('ignores autorepeat, saves once, and resets a shortcut to its default', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record Toggle shortcut' }))
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft' })
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK' })
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK', repeat: true })
    await waitFor(() => expect(vi.mocked(setSettings)).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getAllByText('Ctrl+K').length).toBeGreaterThan(0))

    fireEvent.click(screen.getByRole('button', { name: 'Reset Toggle shortcut' }))
    await waitFor(async () => {
      expect((await getSettings()).toggleShortcut).toEqual({
        value: null,
        effective: 'Super+Alt+Space',
        source: 'default',
      })
    })
  })

  it('rolls back a rejected chord save and leaves the write chain usable', async () => {
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('invalid shortcut'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record Toggle shortcut' }))
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft' })
    fireEvent.keyDown(window, { key: 'x', code: 'KeyX' })
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid shortcut')
    expect(screen.getAllByText('Super+Alt+Space').length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: 'Record Push-to-talk shortcut' }))
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlRight' })
    fireEvent.keyUp(window, { key: 'Control', code: 'ControlRight' })
    await waitFor(() => expect(vi.mocked(setSettings)).toHaveBeenCalledTimes(2))
    expect((await getSettings()).holdKey.effective).toBe('RightCtrl')
  })

  it('locks both shortcut recorders and resets when environment-backed', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      toggleShortcut: { value: 'Ctrl+T', effective: 'Super+Q', source: 'env' },
      holdKey: { value: 'LeftCtrl', effective: 'Alt+Space', source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('ECHO_TOGGLE_SHORTCUT')).toBeInTheDocument()
    expect(screen.getByText('ECHO_HOLD_KEY')).toBeInTheDocument()
    for (const name of [
      'Record Toggle shortcut',
      'Reset Toggle shortcut',
      'Record Push-to-talk shortcut',
      'Reset Push-to-talk shortcut',
    ]) {
      expect(screen.getByRole('button', { name })).toBeDisabled()
    }
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
  })

  it('lists preview microphones and persists a named choice', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const picker = await screen.findByLabelText('Microphone')
    expect(screen.getByRole('option', { name: 'System default' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'USB Microphone' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Built-in Audio Analog Stereo' })).toBeInTheDocument()
    fireEvent.change(picker, { target: { value: 'USB Microphone' } })
    await waitFor(() => expect(picker).toHaveValue('USB Microphone'))
    expect((await getSettings()).microphone).toEqual({
      value: 'USB Microphone',
      effective: 'USB Microphone',
      source: 'file',
    })
  })

  it('names the fallback when the configured microphone is gone', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      microphone: { value: 'Missing Headset', effective: 'Missing Headset', source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Missing Headset is gone; using Built-in Audio Analog Stereo')).toBeInTheDocument()
  })

  it('clears the mic meter when the selection changes', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test' }))
    expect(await screen.findByText('Level 0.042')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Microphone'), { target: { value: 'USB Microphone' } })
    await waitFor(() => expect(screen.queryByText('Level 0.042')).not.toBeInTheDocument())
  })

  it('shows unavailable when a microphone test fails', async () => {
    seedPreviewMicTestError('device busy')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test' }))
    expect(await screen.findByText('Unavailable')).toBeInTheDocument()
  })

  it('hides the Fake engine from the selector by default', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByRole('button', { name: 'Whisper' })
    expect(screen.queryByRole('button', { name: 'Fake' })).not.toBeInTheDocument()
  })

  it('shows the Fake engine when the availability payload includes it', async () => {
    const { listModels } = await import('./tauri')
    const inventory = await listModels()
    const { seedPreviewInventory } = await import('./tauri')
    seedPreviewInventory({
      ...inventory,
      engines: [...inventory.engines, { id: 'fake', available: true, reason: null }],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('button', { name: 'Fake' })).toBeInTheDocument()
  })

  it('shows the model picker while Whisper runs, and hides it for Parakeet', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    // Default fixture: engine auto with Whisper available, so the picker shows.
    const picker = await screen.findByLabelText('Model quality')
    expect(screen.getByRole('option', { name: 'Auto · best installed' })).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'small · multilingual · full precision · 466 MiB' }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'large-v3-turbo-q8_0 · multilingual · q8_0 · 834 MiB' }),
    ).toBeInTheDocument()

    fireEvent.change(picker, { target: { value: 'small' } })
    await waitFor(async () => {
      expect((await getSettings()).whisperModel).toEqual({
        value: 'small',
        effective: 'small',
        source: 'file',
      })
    })

    // The engine override lives in Advanced.
    fireEvent.click(await screen.findByText('Advanced'))
    fireEvent.click(await screen.findByRole('button', { name: 'Parakeet' }))
    await waitFor(() => expect(screen.queryByLabelText('Model quality')).not.toBeInTheDocument())
  })

  it('pins the General surface and keeps Advanced collapsed until asked', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    // General: the four decisions that matter, plus shortcut and theme.
    await screen.findByLabelText('Microphone')
    await screen.findByLabelText('Language')
    await screen.findByLabelText('Model quality')
    await screen.findByLabelText('Push-to-talk shortcut')
    expect(screen.getByRole('group', { name: 'Application theme' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeInTheDocument()

    // Advanced is collapsed by default and expands on click. A details
    // element keeps its children in the DOM; the open attribute is the state.
    const advanced = document.querySelector('.advanced-section')!
    expect(advanced).not.toHaveAttribute('open')
    fireEvent.click(await screen.findByText('Advanced'))
    expect(advanced).toHaveAttribute('open')
    expect(await screen.findByRole('group', { name: 'Speech engine' })).toBeInTheDocument()
    expect(screen.getByText('Resolved engine')).toBeInTheDocument()
  })

  it('marks an unavailable engine with its reason', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findByText('Parakeet: sherpa-onnx-offline is not on PATH'),
    ).toBeInTheDocument()
  })

  it('renders the last-run readout from the fixture', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('whisper-small · 1038 ms')).toBeInTheDocument()
    expect(screen.getByText('/home/user/.cache/echo/ggml-small.bin')).toBeInTheDocument()
    expect(screen.getByText('/usr/local/bin/whisper-cli')).toBeInTheDocument()
    expect(screen.getByText('Yes')).toBeInTheDocument()
    expect(screen.getByText(__APP_VERSION__)).toBeInTheDocument()
  })

  it('surfaces engine stderr on failure', async () => {
    seedPreviewStatus({
      phase: 'Failed speech engine failed',
      lastError: 'whisper-cli: ggml_init failed',
    })
    render(<App />)
    await screen.findByText('Failed speech engine failed')
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('whisper-cli: ggml_init failed')).toBeInTheDocument()
  })

  it('offers Auto first, then a common group, then the alphabetical list', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const picker = await screen.findByLabelText('Language')
    const auto = screen.getByRole('option', { name: 'Auto · detect language' })
    expect(auto).toBeInTheDocument()
    // The common group keeps its fixed order; the full list is alphabetical.
    const common = within(screen.getByRole('group', { name: 'Common' }))
    expect(common.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'English',
      'German',
      'Spanish',
      'French',
    ])
    const all = within(screen.getByRole('group', { name: 'All languages' }))
    const names = all.getAllByRole('option').map((o) => o.textContent)
    expect(names).toEqual([...names].sort((a, b) => a!.localeCompare(b!)))

    fireEvent.change(picker, { target: { value: 'de' } })
    await waitFor(async () => {
      expect((await getSettings()).language).toEqual({
        value: 'de',
        effective: 'de',
        source: 'file',
      })
    })
  })

  it('renders every language a multilingual model offers', async () => {
    const hundred = Array.from({ length: 100 }, (_, i) => ({
      code: `l${i}`,
      englishName: `language ${String(i).padStart(3, '0')}`,
      group: i < 4 ? 'common' : 'all',
    }))
    seedPreviewLanguages({ mode: 'multilingual', model: null, options: hundred })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Language')
    const all = within(screen.getByRole('group', { name: 'All languages' }))
    expect(all.getAllByRole('option')).toHaveLength(100)
    expect(screen.getByRole('option', { name: 'Auto · detect language' })).toBeInTheDocument()
  })

  it('shows the detected-language chip only when Auto is active', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('de · German · p=0.96')).toBeInTheDocument()
  })

  it('hides the detected-language chip when a language is pinned', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'en', effective: 'en', source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Language')
    expect(screen.queryByText('de · German · p=0.96')).not.toBeInTheDocument()
  })

  it('offers to pin a confidently detected language, and pins it', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    // Default fixture: auto active, last run detected German at p=0.96.
    const pin = await screen.findByRole('button', { name: 'Pin German for speed' })
    fireEvent.click(pin)
    await waitFor(async () => {
      expect((await getSettings()).language).toEqual({
        value: 'de',
        effective: 'de',
        source: 'file',
      })
    })
  })

  it('keeps the pin suggestion silent on low confidence', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    seedPreviewStatus({
      lastRun: {
        engine: 'whisper-small',
        binary: null,
        modelPath: null,
        multilingual: true,
        vad: false,
        inferMs: 900,
        language: 'nl',
        languageProbability: 0.31,
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByText('nl · p=0.31')
    expect(screen.queryByRole('button', { name: /Pin .* for speed/ })).not.toBeInTheDocument()
  })

  it('renders low detection confidence differently', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      language: { value: 'auto', effective: 'auto', source: 'file' },
    })
    seedPreviewStatus({
      lastRun: {
        engine: 'whisper-small',
        binary: null,
        modelPath: null,
        multilingual: true,
        vad: false,
        inferMs: 900,
        language: 'nl',
        languageProbability: 0.31,
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const chip = await screen.findByText('nl · p=0.31')
    expect(chip.closest('.status-note')).toHaveAttribute('data-tone', 'attention')
  })

  it('warns before recording when an English-only model meets a non-English choice', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'whisper', effective: 'whisper', source: 'file' },
      language: { value: 'de', effective: 'de', source: 'file' },
    })
    seedPreviewLanguages({ mode: 'english', model: 'ggml-base.en.bin', options: [
      { code: 'en', englishName: 'english', group: 'common' },
    ] })
    seedPreviewStatus({
      languageWarning:
        'ggml-base.en.bin is English-only but the language is set to german. Choose a multilingual model or set the language to English.',
    })
    render(<App />)
    // The warning is on Home before any recording happens.
    expect(
      await screen.findByText(/ggml-base\.en\.bin is English-only but the language is set to german/),
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findAllByText(/ggml-base\.en\.bin is English-only/),
    ).not.toHaveLength(0)
    // An English-only model offers no picker.
    expect(screen.queryByLabelText('Language')).not.toBeInTheDocument()
  })

  it('reports Parakeet as automatic without a picker', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'parakeet', effective: 'parakeet', source: 'file' },
    })
    seedPreviewLanguages({
      mode: 'parakeet',
      model: null,
      options: Array.from({ length: 25 }, (_, i) => ({
        code: `p${i}`,
        englishName: `parakeet language ${i}`,
        group: 'all',
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(
      await screen.findByText('Automatic across 25 languages · not reported'),
    ).toBeInTheDocument()
    expect(screen.queryByLabelText('Language')).not.toBeInTheDocument()
  })

  it('lists uninstalled offers with size and URL, and downloads one', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    // The installed offer renders no download button; uninstalled ones do.
    expect(await screen.findByText('Balanced, multilingual')).toBeInTheDocument()
    expect(screen.getByText(/465 MiB · ~852 MB memory · multilingual/)).toBeInTheDocument()
    expect(screen.getByText('https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin')).toBeInTheDocument()
    expect(screen.queryByText('Fast, English')).not.toBeInTheDocument()

    const row = screen.getByText('Balanced, multilingual').closest('.setting-row')!
    const button = within(row as HTMLElement).getByRole('button', { name: 'Download' })
    fireEvent.click(button)
    expect(
      await within(row as HTMLElement).findByRole('progressbar', {
        name: 'Downloading ggml-small.bin',
      }),
    ).toBeInTheDocument()
    expect(await within(row as HTMLElement).findByText('Verifying…')).toBeInTheDocument()
    expect(await within(row as HTMLElement).findByText('Installed')).toBeInTheDocument()
  })

  it('cancels a download midway', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const row = (await screen.findByText('Best, multilingual')).closest('.setting-row')!
    fireEvent.click(within(row as HTMLElement).getByRole('button', { name: 'Download' }))
    await within(row as HTMLElement).findByRole('progressbar')
    fireEvent.click(within(row as HTMLElement).getByRole('button', { name: 'Cancel' }))
    expect(
      await within(row as HTMLElement).findByRole('button', { name: 'Download' }),
    ).toBeInTheDocument()
  })

  it('offers a multilingual model at the language incompatibility warning', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'whisper', effective: 'whisper', source: 'file' },
      language: { value: 'de', effective: 'de', source: 'file' },
    })
    seedPreviewStatus({
      languageWarning:
        'ggml-base.en.bin is English-only but the language is set to german. Choose a multilingual model or set the language to English.',
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const warning = await screen.findAllByText(/ggml-base\.en\.bin is English-only/)
    const warningRow = warning[0].closest('.setting-row')!
    // The fix is a click, right at the point of failure. The button renders
    // once the async offer fetch resolves, later than the status-driven
    // warning text, so the lookup must wait for it.
    fireEvent.click(
      await within(warningRow as HTMLElement).findByRole('button', { name: 'Download' }),
    )
    expect(
      await within(warningRow as HTMLElement).findByRole('progressbar', {
        name: 'Downloading ggml-small.bin',
      }),
    ).toBeInTheDocument()
    // And the offer is not duplicated in the general list while the warning shows.
    expect(screen.queryByText('Balanced, multilingual')).not.toBeInTheDocument()
  })

  it('offers the VAD model where VAD is reported unavailable', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Silence detection')).toBeInTheDocument()
    expect(screen.getByText(/864 KiB/)).toBeInTheDocument()
  })

  it('shows usage stats derived from history', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const strip = await screen.findByLabelText('Usage')
    expect(within(strip).getByText('words dictated')).toBeInTheDocument()
    // The three fixture rows carry 8 + 9 + 2 words.
    expect(within(strip).getByText('19')).toBeInTheDocument()
    expect(within(strip).getByText('sessions this week')).toBeInTheDocument()
    expect(within(strip).getByText('day streak')).toBeInTheDocument()
  })

  it('verifies only an explicitly attributed shortcut activation', async () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    // Mic and engine are ready in the fixture; only the shortcut remains open.
    const checklist = await screen.findByLabelText('Finish setup')
    expect(within(checklist).getByText('Shortcut bound')).toBeInTheDocument()
    expect(within(checklist).queryByRole('button', { name: 'I bound it' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    // This activation lands after the last app poll but before testing starts.
    // The test's fresh backend baseline must consume it rather than verify it.
    seedPreviewStatus({ shortcutActivation: 'portal:stale-before-click' })
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
    await new Promise((resolve) => window.setTimeout(resolve, 450))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    // An unrelated GUI, tray, or CLI recording phase cannot satisfy the check.
    seedPreviewStatus({ phase: 'Recording', recording: true, recordingInProcess: false })
    await new Promise((resolve) => window.setTimeout(resolve, 30))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    // The configured shortcut path publishes a distinct provenance token.
    seedPreviewStatus({ shortcutActivation: 'portal:1' })
    await waitFor(() => expect(localStorage.getItem('echo-shortcut-verified-at')).not.toBeNull())
    expect(localStorage.getItem('echo-shortcut-verified-identity')).toBe('portal:Super+Alt+Space')
    expect(await screen.findByText(/Verified/)).toBeInTheDocument()

    seedPreviewStatus({ phase: 'Idle', recording: false, recordingInProcess: false })
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    // Everything is green, so the checklist has done its job and is gone.
    await waitFor(() => expect(screen.queryByLabelText('Finish setup')).not.toBeInTheDocument())
  })

  it('leaves the shortcut unverified when no keypress arrives', async () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    // shouldAdvanceTime keeps the app's status poll and waitFor working
    // while the 10 s listener window is jumped over.
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      render(<App />)
      await screen.findByRole('button', { name: 'Start recording' })
      fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
      expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
      await vi.advanceTimersByTimeAsync(10_100)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('lists the microphone step when the mic is not ready', async () => {
    seedPreviewStatus({ microphoneReady: false })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const checklist = await screen.findByLabelText('Finish setup')
    expect(within(checklist).getByText('Microphone ready')).toBeInTheDocument()
    expect(within(checklist).getByText('Speech engine and model installed')).toBeInTheDocument()
  })

  it('parks the level bars for out-of-process sessions and drives them live in-process', async () => {
    seedPreviewStatus({ recording: true, recordingInProcess: false, phase: 'Recording' })
    const { rerender } = render(<App />)
    const parked = await screen.findByText('Listening…')
    expect(parked).toBeInTheDocument()
    const bars = document.querySelector('.level-bars')
    expect(bars).toHaveAttribute('data-live', 'false')

    seedPreviewStatus({ recording: true, recordingInProcess: true, phase: 'Recording' })
    rerender(<App />)
    await waitFor(() => {
      expect(document.querySelector('.level-bars')).toHaveAttribute('data-live', 'true')
    })
    // Live bars get inline heights from the meter.
    await waitFor(() => {
      const heights = [...document.querySelectorAll('.level-bar')].map((bar) =>
        (bar as HTMLElement).style.height,
      )
      expect(heights.some((height) => height && height !== '15%')).toBe(true)
    })
  })

  it('shows native push-to-talk without claiming its evdev listener is unavailable', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Push-to-talk shortcut')
    expect(screen.getByText('Native shortcut active')).toBeInTheDocument()
    expect(screen.queryByText('Listener unavailable')).not.toBeInTheDocument()
    expect(screen.getByText('Desktop portal active')).toBeInTheDocument()
    expect(screen.getByText('Native desktop shortcut active')).toBeInTheDocument()
  })

  it('reports unavailable raw input without requesting privilege changes', async () => {
    seedPreviewStatus({
      holdListener: 'needs-permission',
      holdListenerError: 'Echo cannot read an eligible keyboard; use the desktop global shortcut.',
      shortcutBackend: 'unsupported',
      shortcutHealthy: false,
      legacyShortcut: {
        state: 'ready',
        detail: 'GNOME owns this Echo shortcut and its command is current.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Push-to-talk shortcut')
    expect(
      screen.getByText('Echo cannot read an eligible keyboard; use the desktop global shortcut.'),
    ).toBeInTheDocument()
    expect(screen.queryByText(/sudo|usermod|input group/i)).not.toBeInTheDocument()
    expect(screen.getByText('GNOME shortcut ready')).toBeInTheDocument()
    expect(screen.getByText('GNOME custom shortcut ready')).toBeInTheDocument()
    expect(screen.getByText(/^Raw keyboard input unavailable:/)).toBeInTheDocument()
  })

  it('exposes source-specific shortcut backend diagnostics', async () => {
    seedPreviewStatus({
      holdListener: 'active',
      shortcutBackend: 'x11',
      shortcutHealthy: true,
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(screen.getByText('Toggle shortcut backend')).toBeInTheDocument()
    expect(screen.getByText('X11 active')).toBeInTheDocument()
    expect(screen.getByText('Push-to-talk backend')).toBeInTheDocument()
    expect(screen.getByText('Raw keyboard input active')).toBeInTheDocument()
  })

  it('exposes independent shortcut backend errors', async () => {
    seedPreviewStatus({
      shortcutBackend: 'unsupported',
      shortcutHealthy: false,
      shortcutError: 'Portal registration failed',
      legacyShortcut: null,
      holdListener: 'unavailable',
      holdListenerError: 'No eligible keyboard is connected',
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(screen.getByText('Registration unavailable: Portal registration failed')).toBeInTheDocument()
    expect(screen.getByText('Listener unavailable: No eligible keyboard is connected')).toBeInTheDocument()
  })

  it.each([
    ['missing', 'Set up GNOME shortcut'],
    ['stale', 'Repair GNOME shortcut'],
  ] as const)('requires an explicit action for a %s GNOME shortcut', async (state, action) => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    seedPreviewStatus({
      shortcutHealthy: false,
      shortcutBackend: 'unsupported',
      legacyShortcut: {
        state,
        detail: state === 'missing' ? 'GNOME has no Echo custom shortcut yet.' : 'The Echo command is old.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const repair = await screen.findByRole('button', { name: action })
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(screen.queryByText('Shortcut verified')).not.toBeInTheDocument()

    fireEvent.click(repair)
    await waitFor(() => expect(repairLegacyShortcut).toHaveBeenCalledOnce())
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    expect(localStorage.getItem('echo-shortcut-verified-identity')).toBeNull()
    expect(await screen.findByText('GNOME shortcut ready')).toBeInTheDocument()
  })

  it('does not offer legacy repair for other native backend failures', async () => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    seedPreviewStatus({
      shortcutHealthy: false,
      shortcutBackend: 'unsupported',
      shortcutError: 'portal host registry handshake failed',
      legacyShortcut: null,
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(screen.getByText('setup required in Settings')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Toggle shortcut')

    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('does not reuse verification from a different shortcut identity', async () => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')
    seedPreviewStatus({ shortcut: 'Ctrl+Alt+Space', requestedShortcut: 'Ctrl+Alt+Space' })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Toggle shortcut')

    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(screen.queryByText('Shortcut verified')).not.toBeInTheDocument()
  })

  it('refuses to overwrite a conflicting GNOME shortcut', async () => {
    seedPreviewStatus({
      shortcutHealthy: false,
      legacyShortcut: {
        state: 'conflicting',
        detail: 'Terminal already uses <Super><Alt>space; change it in GNOME Settings first.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Resolve the conflict in GNOME Settings')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('shows ready GNOME setup without offering a write', async () => {
    seedPreviewStatus({
      shortcutHealthy: false,
      legacyShortcut: {
        state: 'ready',
        detail: 'GNOME owns this Echo shortcut and its command is current.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('GNOME shortcut ready')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeEnabled()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('gives unsupported Wayland compositors a truthful manual command', async () => {
    seedPreviewStatus({
      shortcutHealthy: false,
      legacyShortcut: {
        state: 'unsupported',
        detail: 'This Wayland compositor has no GlobalShortcuts portal.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Manual setup required')).toBeInTheDocument()
    expect(screen.getByText('/usr/bin/echo-desktop rec --toggle')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('warns when a stale install shadows the running binary', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    expect(warning).toHaveTextContent('/home/user/.local/bin/echo-desktop')
    expect(warning).toHaveTextContent('rm -f /home/user/.local/bin/echo-desktop')
  })

  it('shows no stale-install warning when PATH is clean', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    await screen.findByLabelText('Finish setup')
    expect(screen.queryByText(/shadows this one/)).not.toBeInTheDocument()
  })

  it('removes stale copies in one click and the warning resolves', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    expect(warning).toHaveTextContent('/home/user/.local/bin/echo-desktop')
    // The manual command stays visible as secondary text.
    expect(warning).toHaveTextContent('rm -f /home/user/.local/bin/echo-desktop')

    fireEvent.click(within(warning).getByRole('button', { name: 'Remove old copies' }))
    expect(vi.mocked(removeStaleInstalls)).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
  })

  it('surfaces a removal failure and keeps the warning', async () => {
    seedPreviewStatus({
      currentExe: '/usr/bin/echo-desktop',
      firstPathHit: '/home/user/.local/bin/echo-desktop',
      staleInstalls: ['/home/user/.local/bin/echo-desktop'],
    })
    seedPreviewRemoveStaleError('permission denied')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    const warning = await screen.findByRole('alert')
    fireEvent.click(within(warning).getByRole('button', { name: 'Remove old copies' }))
    expect(await screen.findByText('permission denied')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toBeInTheDocument()
  })

  it('groups history by day', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'History' }))
    // The fixture rows are all from one fixed day; the header follows the
    // Today / Yesterday / date rule relative to when the test runs.
    const day = new Date(1787310400 * 1000)
    day.setHours(0, 0, 0, 0)
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000)
    const expected =
      diffDays === 0
        ? 'Today'
        : diffDays === 1
          ? 'Yesterday'
          : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(day)
    const headers = await screen.findAllByRole('heading', { level: 3 })
    expect(headers.map((header) => header.textContent)).toContain(expected)
  })
})
