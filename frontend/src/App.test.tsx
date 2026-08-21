import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import App from './App'
import { getSettings, resetPreviewSettings, seedPreviewSettings } from './tauri'

describe('Echo desktop shell', () => {
  beforeEach(() => {
    resetPreviewSettings()
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
})
