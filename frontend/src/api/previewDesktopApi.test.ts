import { createPreviewDesktopApi } from './previewDesktopApi'

describe('preview desktop adapter contract', () => {
  it('provides the complete setup and microphone fixtures', async () => {
    const preview = createPreviewDesktopApi()
    const readiness = await preview.getReadiness()
    const microphones = await preview.getMicrophones()

    expect(readiness.components.map(({ id }) => id)).toEqual([
      'whisper-runtime',
      'whisper-base-q51',
      'whisper-small',
      'whisper-large-v3-turbo-q50',
      'silero-vad',
      'whisper-vulkan-runtime',
      'sherpa-runtime',
      'parakeet-tdt06b-v3-int8',
    ])
    expect(readiness.plans.map(({ id }) => id)).toEqual([
      'recommended',
      'parakeet',
      'whisper-base',
      'whisper-small',
      'whisper-large-v3-turbo',
    ])
    expect(readiness.components.find(({ id }) => id === 'whisper-vulkan-runtime')?.managed)
      .toEqual({ kind: 'absent', resumableBytes: 0 })
    expect(microphones.devices.some(({ label }) => label === 'Jabra Elite 8 Active')).toBe(true)
    expect(microphones.devices.filter(({ tier }) => tier === 'advanced')).toHaveLength(8)
  })

  it('keeps mutable fixtures isolated per adapter', async () => {
    const first = createPreviewDesktopApi()
    const second = createPreviewDesktopApi()

    await first.addDictionaryEntry('new phrase', 'New phrase')

    expect(await first.getDictionary()).toHaveLength(3)
    expect(await second.getDictionary()).toHaveLength(2)
  })

  it('cancels pending setup work when reset', async () => {
    vi.useFakeTimers()
    try {
      const preview = createPreviewDesktopApi()
      const handler = vi.fn()
      await preview.onSetupEvent(handler)
      await preview.startSetup('recommended')
      preview.resetPreviewSettings()

      await vi.runAllTimersAsync()
      expect(handler).toHaveBeenCalledTimes(1)
      expect((await preview.getReadiness()).activeOperation).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not finish setup after cancellation', async () => {
    vi.useFakeTimers()
    try {
      const preview = createPreviewDesktopApi()
      const handler = vi.fn()
      await preview.onSetupEvent(handler)
      const operation = await preview.startSetup('recommended')
      await preview.cancelSetup(operation)

      await vi.runAllTimersAsync()
      expect(handler.mock.calls.map(([event]) => event.kind)).toEqual(['progress', 'cancelled'])
    } finally {
      vi.useRealTimers()
    }
  })
})
