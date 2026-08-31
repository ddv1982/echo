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

  it('runs five deterministic voice samples and saves them in one batch', async () => {
    const preview = createPreviewDesktopApi()
    const samples = []
    for (let take = 0; take < 5; take += 1) {
      const capture = await preview.startDictionaryTrainingSample()
      samples.push(await preview.finishDictionaryTrainingSample(capture))
    }

    expect(samples.map(({ transcript }) => transcript)).toEqual([
      'kuber netties',
      'cooper net ease',
      'Kubernetes',
      'kuber netties',
      'cube er netties',
    ])
    expect(new Set(samples.map(({ engine }) => engine))).toEqual(new Set(['whisper-small']))

    const result = await preview.addDictionaryEntriesBatch(
      'Kubernetes',
      samples.map(({ transcript }) => transcript),
    )
    expect(result).toMatchObject({ added: 3, unchanged: 2, conflicts: [] })
    expect(result.entries.filter(({ written }) => written === 'Kubernetes')).toHaveLength(3)
  })

  it('rolls back a preview batch when a spoken form conflicts', async () => {
    const preview = createPreviewDesktopApi()
    const before = await preview.getDictionary()
    const result = await preview.addDictionaryEntriesBatch('Different', ['new phrase', 'post grass'])

    expect(result.added).toBe(0)
    expect(result.conflicts).toEqual([{ spoken: 'post grass', written: 'Postgres' }])
    expect(await preview.getDictionary()).toEqual(before)
  })

  it('returns detached nested snapshots', async () => {
    const preview = createPreviewDesktopApi()
    const readiness = await preview.getReadiness()
    const microphones = await preview.getMicrophones()
    const inventory = await preview.listModels()
    const settings = await preview.getSettings()
    const status = await preview.getAppStatus()
    const systemDefault = microphones.systemDefault
    const performance = status.lastRun?.performance
    if (!systemDefault || !performance) throw new Error('rich preview fixtures are incomplete')

    readiness.components[0].external[0].path = '/mutated/runtime'
    readiness.plans[0].components.push('sherpa-runtime')
    systemDefault.label = 'Mutated microphone'
    microphones.devices[0].extended.push('mutated')
    inventory.whisper[0].name = 'mutated-model'
    settings.preferences.engine.effective = 'mutated-engine'
    status.recordingPolicy.presetsSeconds[0] = 999
    performance.tuning.threads = 99

    expect((await preview.getReadiness()).components[0].external[0].path)
      .toBe('/usr/bin/whisper-cli')
    expect((await preview.getReadiness()).plans[0].components).not.toContain('sherpa-runtime')
    expect((await preview.getMicrophones()).systemDefault?.label).toBe('System default')
    expect((await preview.getMicrophones()).devices[0].extended).toEqual([])
    expect((await preview.listModels()).whisper[0].name).toBe('base.en-q5_1')
    expect((await preview.getSettings()).preferences.engine.effective).toBe('auto')
    expect((await preview.getAppStatus()).recordingPolicy.presetsSeconds[0]).toBe(30)
    expect((await preview.getAppStatus()).lastRun?.performance?.tuning.threads).toBe(4)
  })

  it('copies nested seed inputs', async () => {
    const source = createPreviewDesktopApi()
    const preview = createPreviewDesktopApi()
    const readiness = await source.getReadiness()
    const microphones = await source.getMicrophones()
    const inventory = await source.listModels()
    const settings = await source.getSettings()
    const status = source.richPreviewStatus()
    const systemDefault = microphones.systemDefault
    const performance = status.lastRun?.performance
    if (!systemDefault || !performance) throw new Error('rich preview fixtures are incomplete')

    preview.seedPreviewReadiness(readiness)
    preview.seedPreviewMicrophones(microphones)
    preview.seedPreviewInventory(inventory)
    preview.seedPreviewSettings(settings.preferences)
    preview.seedPreviewStatus(status)

    readiness.components[0].external[0].path = '/mutated/runtime'
    systemDefault.label = 'Mutated microphone'
    inventory.whisper[0].name = 'mutated-model'
    settings.preferences.engine.effective = 'mutated-engine'
    status.recordingPolicy.presetsSeconds[0] = 999
    performance.tuning.threads = 99

    expect((await preview.getReadiness()).components[0].external[0].path)
      .toBe('/usr/bin/whisper-cli')
    expect((await preview.getMicrophones()).systemDefault?.label).toBe('System default')
    expect((await preview.listModels()).whisper[0].name).toBe('base.en-q5_1')
    expect((await preview.getSettings()).preferences.engine.effective).toBe('auto')
    expect((await preview.getAppStatus()).recordingPolicy.presetsSeconds[0]).toBe(30)
    expect((await preview.getAppStatus()).lastRun?.performance?.tuning.threads).toBe(4)
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

  it('rejects a second setup start without orphaning completion work', async () => {
    vi.useFakeTimers()
    try {
      const preview = createPreviewDesktopApi()
      const handler = vi.fn()
      await preview.onSetupEvent(handler)
      const operation = await preview.startSetup('parakeet')

      await expect(preview.startSetup('parakeet')).rejects.toThrow('setup operation already in progress')
      await preview.cancelSetup(operation)
      await vi.runAllTimersAsync()

      expect(handler.mock.calls.map(([event]) => event.kind)).toEqual(['progress', 'cancelled'])
      expect((await preview.getReadiness()).plans.find(({ id }) => id === 'parakeet')?.satisfied)
        .toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })
})
