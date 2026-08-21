import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import App from './App'
import { getSettings, resetPreviewSettings, seedPreviewSettings, setSettings } from './tauri'

vi.mock('./tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./tauri')>()
  return {
    ...actual,
    setSettings: vi.fn((settings) => actual.setSettings(settings)),
  }
})

describe('Echo desktop shell', () => {
  beforeEach(async () => {
    resetPreviewSettings()
    const actual = await vi.importActual<typeof import('./tauri')>('./tauri')
    vi.mocked(setSettings).mockReset()
    vi.mocked(setSettings).mockImplementation((settings) => actual.setSettings(settings))
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
    expect(await screen.findByRole('button', { name: 'Stop & transcribe' })).toBeInTheDocument()

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
    fireEvent.click(await screen.findByRole('button', { name: 'Fake' }))
    expect(await screen.findByText('Fake test engine', { selector: '.setting-line span' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Fake' })).toHaveAttribute('data-active', 'true')
    expect((await getSettings()).engine).toEqual({ value: 'fake', effective: 'fake', source: 'file' })

    fireEvent.change(screen.getByLabelText('Hold key'), { target: { value: 'RightShift' } })
    await waitFor(() => expect(screen.getByLabelText('Hold key')).toHaveValue('RightShift'))
    expect((await getSettings()).holdKey).toEqual({
      value: 'RightShift',
      effective: 'RightShift',
      source: 'file',
    })
  })

  it('disables an env-backed field and names the variable', async () => {
    const defaults = await getSettings()
    seedPreviewSettings({
      ...defaults,
      engine: { value: null, effective: 'fake', source: 'env' },
    })
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    const fake = await screen.findByRole('button', { name: 'Fake' })
    expect(fake).toBeDisabled()
    expect(fake).toHaveAttribute('data-active', 'true')
    expect(screen.getByText('ECHO_ENGINE')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Whisper' }))
    expect((await getSettings()).engine.effective).toBe('fake')
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
    fireEvent.click(await screen.findByRole('button', { name: 'Fake' }))
    await firstWriteStarted
    fireEvent.change(screen.getByLabelText('Hold key'), { target: { value: 'RightShift' } })
    releaseFirst()

    await waitFor(async () => {
      const stored = await getSettings()
      expect(stored.engine).toEqual({ value: 'fake', effective: 'fake', source: 'file' })
      expect(stored.holdKey).toEqual({
        value: 'RightShift',
        effective: 'RightShift',
        source: 'file',
      })
    })
  })

  it('shows the error banner when a settings save fails', async () => {
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('could not write settings'))
    render(<App />)
    await screen.findByRole('button', { name: 'Start recording' })
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Fake' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('could not write settings')
    expect(screen.getByRole('button', { name: 'Fake' })).toHaveAttribute('data-active', 'false')
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
})
