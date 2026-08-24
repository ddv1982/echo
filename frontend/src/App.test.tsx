import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import App from './App'
import {
  getSettings,
  getMicrophones,
  getReadiness,
  listModels,
  removeStaleInstalls,
  repairLegacyShortcut,
  resetPreviewSettings,
  retryShortcut,
  seedPreviewLanguages,
  seedPreviewLanguagesError,
  seedPreviewMicrophones,
  seedPreviewMicTestError,
  seedPreviewRemoveStaleError,
  seedPreviewReadiness,
  seedPreviewSettings,
  seedPreviewStatus,
  setSettings,
  setMicrophone,
  stopRecording,
} from './tauri'
import type { ShortcutStatus } from './types'

vi.mock('./tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./tauri')>()
  return {
    ...actual,
    getSettings: vi.fn(() => actual.getSettings()),
    getMicrophones: vi.fn(() => actual.getMicrophones()),
    listModels: vi.fn(() => actual.listModels()),
    setSettings: vi.fn((settings) => actual.setSettings(settings)),
    setMicrophone: vi.fn((id) => actual.setMicrophone(id)),
    removeStaleInstalls: vi.fn(() => actual.removeStaleInstalls()),
    repairLegacyShortcut: vi.fn(() => actual.repairLegacyShortcut()),
    retryShortcut: vi.fn(() => actual.retryShortcut()),
    stopRecording: vi.fn((activation) => actual.stopRecording(activation)),
  }
})

function activeShortcut(
  activation: string | null = null,
  effective = 'Super+Alt+Space',
): ShortcutStatus {
  return {
    kind: 'active',
    desired: 'Super+Alt+Space',
    effective,
    backend: 'portal',
    activation,
    verificationIdentity: `portal:${effective}`,
  }
}

describe('Echo desktop shell', () => {
  beforeEach(async () => {
    resetPreviewSettings()
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((settings) => actual.setSettings(settings))
    vi.mocked(repairLegacyShortcut).mockReset()
    vi.mocked(repairLegacyShortcut).mockImplementation(() => actual.repairLegacyShortcut())
    vi.mocked(retryShortcut).mockReset()
    vi.mocked(retryShortcut).mockImplementation(() => actual.retryShortcut())
    vi.mocked(stopRecording).mockReset()
    vi.mocked(stopRecording).mockImplementation((activation) => actual.stopRecording(activation))
    vi.mocked(getSettings).mockReset()
    vi.mocked(getSettings).mockImplementation(() => actual.getSettings())
    vi.mocked(getMicrophones).mockReset()
    vi.mocked(getMicrophones).mockImplementation(() => actual.getMicrophones())
    vi.mocked(setMicrophone).mockReset()
    vi.mocked(setMicrophone).mockImplementation((id) => actual.setMicrophone(id))
    vi.mocked(listModels).mockReset()
    vi.mocked(listModels).mockImplementation(() => actual.listModels())
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
  })

  it('offers the Rust recording policy in General and clears the default override', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const select = await screen.findByLabelText('Maximum recording length')
    expect(within(select).getAllByRole('option').map((option) => option.textContent)).toEqual([
      '30 seconds',
      '1 minute',
      '2 minutes',
      '5 minutes',
      '10 minutes · Default',
    ])
    expect(select).toHaveValue('default')

    fireEvent.change(select, { target: { value: '120' } })
    await waitFor(async () => expect((await getSettings()).recordSeconds.value).toBe(120))
    fireEvent.change(select, { target: { value: 'default' } })
    await waitFor(async () => expect((await getSettings()).recordSeconds.value).toBeNull())
  })

  it('preserves a custom recording limit and locks an environment override', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: 90, effective: 90, source: 'file' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('option', { name: '90 seconds' })).toBeInTheDocument()
    expect(screen.getByLabelText('Maximum recording length')).toHaveValue('90')

    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: 30, effective: 90, source: 'env' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Home' }))
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const locked = await screen.findByLabelText('Maximum recording length')
    expect(locked).toBeDisabled()
    expect(locked).toHaveValue('90')
    expect(screen.getByText('ECHO_RECORD_SECONDS')).toBeInTheDocument()
  })

  it('shows an explicit 600-second environment override as selected', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      recordSeconds: { value: null, effective: 600, source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    const select = await screen.findByLabelText('Maximum recording length')
    expect(select).toHaveValue('600')
    expect(within(select).getByRole('option', { name: '10 minutes' })).toBeInTheDocument()
  })

  it('uses the snapped recording limit on Home', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      recordingInProcess: false,
      recordingLimitSeconds: 120,
    })
    render(<App />)
    expect(await screen.findByText('0:00 / 2:00')).toBeInTheDocument()
  })

  it('does not invent a limit for a legacy active status', async () => {
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      recordingInProcess: false,
      recordingLimitSeconds: null,
    })
    render(<App />)

    expect(await screen.findByText('0:00')).toBeInTheDocument()
    expect(screen.queryByText(/0:00 \/ /)).not.toBeInTheDocument()
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
    fireEvent.click(within(screen.getByRole('group', { name: 'Recording HUD' })).getByRole('button', { name: 'Off' }))
    releaseFirst()

    await waitFor(async () => {
      const stored = await getSettings()
      expect(stored.cleanup).toEqual({ value: 'off', effective: 'off', source: 'file' })
      expect(stored.hud).toEqual({ value: false, effective: false, source: 'file' })
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

  it('shows one fixed toggle shortcut with no customization controls', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findAllByText('Super+Alt+Space')).not.toHaveLength(0)
    expect(screen.queryByRole('button', { name: /Record .*shortcut/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Reset .*shortcut/ })).not.toBeInTheDocument()
    expect(screen.queryByText(/Push-to-talk/i)).not.toBeInTheDocument()
  })

  it('keeps shortcut setup available when editable settings fail to load', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('settings unavailable'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findAllByText('Toggle shortcut')).not.toHaveLength(0)
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeEnabled()
    expect(await screen.findByRole('alert')).toHaveTextContent('settings unavailable')
  })


  it('renders duplicate labels as distinct stable-ID radio rows', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const choices = await screen.findAllByRole('radio', { name: /USB Microphone/ })
    expect(choices).toHaveLength(2)
    expect(screen.getByText('USB · Microphone · Focusrite')).toBeInTheDocument()
    expect(screen.getByText('USB · Headset · Logitech')).toBeInTheDocument()
    fireEvent.click(choices[1])
    await waitFor(async () => {
      const snapshot = await getMicrophones()
      expect(snapshot.selection).toMatchObject({
        kind: 'selected',
        device: { id: 'pipewire:alsa_input.usb-Logitech_USB_Headset-00.mono-fallback' },
      })
    })
  })

  it('shows recognizable sources before collapsed technical endpoints', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    expect(await screen.findByRole('radio', { name: /Jabra Elite 8 Active/ })).toBeVisible()
    expect(screen.getByText('Bluetooth · Headset · Jabra')).toBeVisible()
    expect(screen.getByText('Follows the current Linux input automatically')).toBeVisible()
    expect(screen.queryByText('pipewire:input_default')).not.toBeInTheDocument()
    const advanced = screen.getByText('Advanced audio endpoints').closest('details')!
    expect(advanced).not.toHaveAttribute('open')
    expect(screen.getByText('PipeWire Sound Server')).not.toBeVisible()
    fireEvent.click(screen.getByText('Advanced audio endpoints'))
    expect(screen.getByText('PipeWire Sound Server')).toBeVisible()
    expect(screen.getByText('alsa:pipewire')).toBeVisible()
  })

  it('names the active input when Linux has no declared default', async () => {
    const snapshot = await getMicrophones()
    const active = snapshot.devices[0]
    seedPreviewMicrophones({
      ...snapshot,
      systemDefault: null,
      systemDefaultIsProxy: false,
      selection: { kind: 'system-default', active },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText(`Using ${active.label} because Linux has no default input`)).toBeVisible()
  })

  it('names a missing selection and tests fallback only through the explicit action', async () => {
    const snapshot = await getMicrophones()
    seedPreviewMicrophones({
      ...snapshot,
      source: 'config',
      selection: {
        kind: 'missing-with-fallback',
        requestedId: 'alsa:travel',
        requestedLabel: 'Travel Mic',
        fallback: snapshot.systemDefault!,
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText(/Travel Mic is disconnected/)).toBeInTheDocument()
    expect(screen.getByText(/current input from Linux Sound Settings/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Test selected' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Test system fallback' }))
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Input heard on System default',
    )
  })

  it('clears the microphone test result when the selection changes', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test selected' }))
    expect(await screen.findByRole('status')).toHaveTextContent('Input heard')
    fireEvent.click((await screen.findAllByRole('radio', { name: /USB Microphone/ }))[0])
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
  })

  it('announces the exact microphone test failure', async () => {
    seedPreviewMicTestError('device busy')
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test selected' }))
    expect(await screen.findByRole('status')).toHaveTextContent('device busy')
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

  it('shows the Whisper picker and the fixed Parakeet speech model', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    // Default fixture: engine auto with Whisper available, so the picker shows.
    const picker = await screen.findByLabelText('Speech model')
    expect(screen.getByRole('option', { name: 'Auto · best installed' })).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'Lower memory, lower accuracy · small · multilingual · full precision · 466 MiB' }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('option', { name: 'Recommended balance · large-v3-turbo-q8_0 · multilingual · q8_0 · 834 MiB' }),
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
    await waitFor(() => expect(screen.queryByLabelText('Speech model')).not.toBeInTheDocument())
    expect(screen.getByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    await waitFor(async () => expect((await getSettings()).whisperModel.value).toBeNull())
  })

  it('projects Auto as Parakeet when the backend language mode resolves Parakeet', async () => {
    seedPreviewLanguages({
      mode: 'parakeet',
      model: null,
      options: Array.from({ length: 25 }, (_, index) => ({
        code: `p${index}`,
        englishName: `parakeet language ${index}`,
        group: 'all',
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    expect(screen.queryByLabelText('Speech model')).not.toBeInTheDocument()
  })

  it('honors a backend Whisper projection over a file-backed Parakeet choice', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      engine: { value: 'parakeet', effective: 'parakeet', source: 'file' },
      whisperModel: { value: null, effective: 'small', source: 'env' },
    })
    seedPreviewLanguages({
      mode: 'multilingual',
      model: 'small',
      options: [{ code: 'en', englishName: 'english', group: 'common' }],
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByLabelText('Speech model')).toBeDisabled()
    expect(screen.queryByText('Parakeet TDT 0.6B v3')).not.toBeInTheDocument()
  })

  it('shows a saved engine change when its projection refresh fails', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByLabelText('Speech model')
    seedPreviewLanguagesError('language projection failed')
    fireEvent.click(await screen.findByText('Advanced'))
    const parakeet = await screen.findByRole('button', { name: 'Parakeet' })
    fireEvent.click(parakeet)
    await waitFor(() => expect(parakeet).toHaveAttribute('data-active', 'true'))
    expect(screen.queryByLabelText('Speech model')).not.toBeInTheDocument()
    expect(await screen.findByText('language projection failed')).toBeInTheDocument()
  })

  it('pins the General surface and keeps Advanced collapsed until asked', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))

    await screen.findByLabelText('Microphone')
    await screen.findByLabelText('Language')
    await screen.findByLabelText('Speech model')
    expect(await screen.findAllByText('Super+Alt+Space')).not.toHaveLength(0)
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
    expect(screen.getByText('Cold CLI')).toBeInTheDocument()
    expect(screen.getByText('System · CPU')).toBeInTheDocument()
    expect(screen.getByText('WAV 3 ms · process 1032 ms · parse 1 ms')).toBeInTheDocument()
    expect(screen.getByText('4 threads · beam 5 · best of 5 · fallback on')).toBeInTheDocument()
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

  it('reports Parakeet as an automatic fixed speech model', async () => {
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
    expect(screen.getByText('Parakeet TDT 0.6B v3')).toBeInTheDocument()
    expect(screen.queryByLabelText('Language')).not.toBeInTheDocument()
  })

  it('keeps component paths collapsed until Installed components opens', async () => {
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Ready to dictate')).toBeInTheDocument()
    const components = screen.getByText('Installed components').closest('details')!
    expect(components).not.toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).not.toBeVisible()
    fireEvent.click(screen.getByText('Installed components'))
    expect(components).toHaveAttribute('open')
    expect(screen.getByText(/System · \/usr\/bin\/whisper-cli/)).toBeVisible()
  })

  it('runs recommended setup and refreshes terminal state', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: false, downloadBytes: 498_000_000 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: /Set up Large v3 Turbo Q5_0/ }))
    await waitFor(async () => {
      expect((await getReadiness()).plans.find((plan) => plan.id === 'recommended')?.satisfied).toBe(true)
    })
  })

  it('can activate an already available recommended setup', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: true, downloadBytes: 0 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Use Large v3 Turbo Q5_0' }))
    await waitFor(async () => expect((await getReadiness()).speechReady).toBe(true))
  })

  it('does not start setup while a non-cancellable operation is active', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      activeOperation: 'verify-whisper',
      activeCancellable: false,
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByRole('heading', { name: 'Settings' })
    expect(screen.queryByRole('button', { name: /recommended setup/i })).not.toBeInTheDocument()
  })

  it('offers Use for an already available alternative without duplicating Recommended', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      plans: readiness.plans.map((plan) =>
        plan.id === 'parakeet' ? { ...plan, satisfied: true } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Advanced speech options'))
    const advanced = screen.getByText('Advanced speech options').closest('details')!
    const parakeetRow = within(advanced).getByText('Parakeet').closest<HTMLElement>('.setting-row')!
    expect(within(parakeetRow).getByRole('button', { name: 'Use' })).toBeEnabled()
    expect(within(advanced).queryByText('Whisper Large v3 Turbo Q5_0')).not.toBeInTheDocument()
  })

  it('shows repair and managed-only removal actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      components: readiness.components.map((component) =>
        component.id === 'silero-vad'
          ? { ...component, managed: { kind: 'needs-repair', reason: 'wrong size', resumableBytes: 0 } }
          : component.id === 'whisper-small'
            ? { ...component, managed: { kind: 'ready', version: 'small', bytes: 100, root: '/managed/small' } }
            : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByRole('button', { name: 'Repair' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Remove ·/ })).toBeInTheDocument()
  })

  it('shows unsupported guidance without managed mutation actions', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      managedSupported: false,
      speechReady: false,
      unsupportedReason: 'Managed setup is available on Linux x86_64.',
      components: readiness.components.map((component) => ({
        ...component,
        managed: { kind: 'unsupported', reason: 'Linux x86_64 only' },
      })),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByText('Installed components'))
    expect(await screen.findByText('Managed setup is available on Linux x86_64.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Set up Large v3 Turbo Q5_0/ })).not.toBeInTheDocument()
  })

  it('renders component progress, resume, and low-space admission truthfully', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      speechReady: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended'
          ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 900 bytes free; 400 bytes are available' }
          : plan.id === 'parakeet'
            ? { ...plan, satisfied: false, diskReady: false, diskReason: 'Needs 1200 bytes free; 400 bytes are available' }
            : plan,
      ),
      components: readiness.components.map((component) =>
        component.id === 'whisper-large-v3-turbo-q5-0'
          ? {
              ...component,
              managed: { kind: 'absent', resumableBytes: 42 },
              activity: {
                operationId: 'op-1',
                component: component.id,
                phase: 'downloading',
                receivedBytes: 50,
                totalBytes: 100,
                resumedFromBytes: 42,
              },
            }
          : component,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('progressbar', { name: 'Large v3 Turbo Q5_0 downloading' })).toHaveAttribute('aria-valuenow', '50')
    expect(screen.getByRole('button', { name: /Resume/ })).toBeInTheDocument()
    expect(screen.getByText('Recommended: Needs 900 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByText('Parakeet: Needs 1200 bytes free; 400 bytes are available')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Set up Large v3 Turbo Q5_0/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Use Parakeet instead' })).toBeDisabled()
  })

  it('shows microphone then speech as guided Home steps', async () => {
    const readiness = await getReadiness()
    seedPreviewReadiness({
      ...readiness,
      microphoneReady: false,
      speechReady: false,
      hasSuccessfulDictation: false,
      firstRunComplete: false,
      plans: readiness.plans.map((plan) =>
        plan.id === 'recommended' ? { ...plan, satisfied: false, downloadBytes: 100 } : plan,
      ),
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(await screen.findByText('1 · Choose and test a microphone')).toBeInTheDocument()
    expect(screen.getByText('2 · Set up local speech')).toBeInTheDocument()
    expect(screen.getByText('First dictation complete')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: /USB Microphone.*Focusrite/ }))
    await waitFor(() => {
      expect(screen.queryByText('1 · Choose and test a microphone')).not.toBeInTheDocument()
    })
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
    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:stale-before-click') })
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
    await new Promise((resolve) => window.setTimeout(resolve, 450))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    // An unrelated GUI, tray, or CLI recording phase cannot satisfy the check.
    seedPreviewStatus({ phase: 'Recording', recording: true, recordingInProcess: false })
    await new Promise((resolve) => window.setTimeout(resolve, 30))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('toggle-command:1') })
    await new Promise((resolve) => window.setTimeout(resolve, 150))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:changed', 'Alt+F8') })
    await new Promise((resolve) => window.setTimeout(resolve, 150))
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:1') })
    await waitFor(() => expect(localStorage.getItem('echo-shortcut-verified-at')).not.toBeNull())
    expect(stopRecording).toHaveBeenCalledWith('native-toggle:1')
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
      seedPreviewStatus({ phase: 'Recording', recording: true, recordingInProcess: true })
      await vi.advanceTimersByTimeAsync(10_100)
      expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
      expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
      expect(stopRecording).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not verify a shortcut when recording cleanup fails', async () => {
    vi.mocked(stopRecording).mockRejectedValueOnce(new Error('cannot stop recording'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()

    seedPreviewStatus({ shortcut: activeShortcut('native-toggle:cleanup-failure') })
    await waitFor(() => expect(stopRecording).toHaveBeenCalledOnce())
    expect(localStorage.getItem('echo-shortcut-verified-at')).toBeNull()
    expect(await screen.findByText('No keypress seen — check the binding')).toBeInTheDocument()
  })

  it('stops only an attributed shortcut-test recording when Settings unmounts', async () => {
    const { unmount } = render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Test shortcut' }))
    expect(await screen.findByText('Listening… press your shortcut')).toBeInTheDocument()
    seedPreviewStatus({
      phase: 'Recording',
      recording: true,
      shortcut: activeShortcut('native-toggle:unmount'),
    })

    unmount()
    await waitFor(() =>
      expect(stopRecording).toHaveBeenCalledWith('native-toggle:unmount'),
    )
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

  it('renders shortcut setup when settings, models, and microphones fail to load', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('settings unavailable'))
    vi.mocked(listModels).mockRejectedValueOnce(new Error('models unavailable'))
    vi.mocked(getMicrophones).mockRejectedValueOnce(new Error('microphones unavailable'))
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state: 'missing',
          detail: 'GNOME has no Echo custom shortcut yet.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByRole('button', { name: 'Set up GNOME shortcut' })).toBeInTheDocument()
    expect(screen.getAllByText('Super+Alt+Space')).not.toHaveLength(0)
  })


  it.each([
    ['missing', 'Set up GNOME shortcut'],
    ['stale', 'Repair GNOME shortcut'],
  ] as const)('requires an explicit action for a %s GNOME shortcut', async (state, action) => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state,
          detail: state === 'missing' ? 'GNOME has no Echo custom shortcut yet.' : 'The Echo command is old.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
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
      shortcut: {
        kind: 'failed',
        desired: 'Super+Alt+Space',
        detail: 'portal host registry handshake failed',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(screen.getByText('setup required in Settings')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await screen.findByText('Shortcut unavailable')

    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('retries an operational shortcut failure explicitly', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'failed',
        desired: 'Super+Alt+Space',
        detail: 'portal shortcut listener stopped',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Retry shortcut' }))
    await waitFor(() => expect(retryShortcut).toHaveBeenCalledOnce())
  })

  it('does not reuse verification from a different shortcut identity', async () => {
    localStorage.setItem('echo-shortcut-verified-at', '1710000000')
    localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')
    seedPreviewStatus({ shortcut: activeShortcut(null, 'Ctrl+Alt+Space') })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findAllByText('Ctrl+Alt+Space')).not.toHaveLength(0)

    expect(screen.queryByText(/^Verified /)).not.toBeInTheDocument()
    expect(screen.queryByText('Shortcut verified')).not.toBeInTheDocument()
  })

  it('refuses to overwrite a conflicting GNOME shortcut', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state: 'conflicting',
          detail: 'Terminal already uses <Super><Alt>space; change it in GNOME Settings first.',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Resolve the GNOME conflict')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /GNOME shortcut/ })).not.toBeInTheDocument()
    expect(repairLegacyShortcut).not.toHaveBeenCalled()
  })

  it('shows ready GNOME setup without offering a write', async () => {
    seedPreviewStatus({
      shortcut: {
        kind: 'gnome-ready',
        desired: 'Super+Alt+Space',
        effective: 'Super+Alt+Space',
        detail: 'GNOME owns this Echo shortcut and its command is current.',
        command: '/usr/bin/echo-desktop rec --toggle',
        binding: '<Super><Alt>space',
        activation: null,
        verificationIdentity: 'gnome:<Super><Alt>space:/usr/bin/echo-desktop rec --toggle',
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
      shortcut: {
        kind: 'manual',
        desired: 'Super+Alt+Space',
        detail: 'This Wayland compositor has no GlobalShortcuts portal.',
        command: '/usr/bin/echo-desktop rec --toggle',
      },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    expect(await screen.findByText('Bind it in your desktop settings.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByText('Manual shortcut setup')).toBeInTheDocument()
    expect(screen.getByText('/usr/bin/echo-desktop rec --toggle')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Test shortcut' })).toBeDisabled()
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
