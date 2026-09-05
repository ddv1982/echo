import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SettingsChange, SetupEvent } from '../generated/ipc'
import { createTauriDesktopApi, tauriDesktopApi } from './tauriDesktopApi'

const { TestChannel, invokeMock, listenMock } = vi.hoisted(() => {
  class HoistedTestChannel<T> {
    onmessage: (message: T) => void

    constructor(onmessage: (message: T) => void) {
      this.onmessage = onmessage
    }
  }
  return {
    TestChannel: HoistedTestChannel,
    invokeMock: vi.fn<(command: string, commandArguments?: unknown) => Promise<void>>(
      (command, commandArguments) => {
        void command
        void commandArguments
        return Promise.resolve()
      },
    ),
    listenMock: vi.fn((event: string, handler: (event: { payload: SetupEvent }) => void) => {
      void event
      void handler
      return Promise.resolve(() => undefined)
    }),
  }
})

vi.mock('@tauri-apps/api/core', () => ({ Channel: TestChannel, invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

function requireFixture<T>(value: T | undefined, description: string): T {
  if (value === undefined) throw new Error(`missing test fixture: ${description}`)
  return value
}

function resolveLastChannel(message: unknown): void {
  const call = requireFixture(invokeMock.mock.calls.at(-1), 'queued command invocation')
  const args = call[1]
  if (!hasReply(args)) {
    throw new Error('queued command invocation did not include a reply channel')
  }
  args.reply.onmessage(message)
}

function hasReply(value: unknown): value is { reply: InstanceType<typeof TestChannel> } {
  return typeof value === 'object' && value != null && 'reply' in value && value.reply instanceof TestChannel
}

function queuedArgs(command: string): unknown {
  const call = invokeMock.mock.calls.find(([called]) => called === command)
  return requireFixture(call, `${command} invocation`)[1]
}

function queuedReply(command: string): InstanceType<typeof TestChannel> {
  const args = queuedArgs(command)
  if (!hasReply(args)) throw new Error(`${command} invocation did not include a reply channel`)
  return args.reply
}

describe('Tauri desktop adapter contract', () => {
  beforeEach(() => {
    invokeMock.mockClear()
    listenMock.mockClear()
  })

  it('uses the exact command names and argument keys', async () => {
    const change: SettingsChange = { kind: 'whisperAcceleration', value: 'gpu' }
    await tauriDesktopApi.getAppStatus()
    await tauriDesktopApi.getShortcutStatus()
    await tauriDesktopApi.retryShortcut()
    await tauriDesktopApi.repairLegacyShortcut()
    await tauriDesktopApi.getHistory()
    await tauriDesktopApi.deleteHistoryItem('history-id')
    await tauriDesktopApi.clearHistory()
    await tauriDesktopApi.getDictionary()
    await tauriDesktopApi.addDictionaryEntry('spoken', 'written')
    await tauriDesktopApi.addDictionaryEntriesBatch('written', ['first', 'second'])
    await tauriDesktopApi.removeDictionaryEntry('spoken', 'written')
    const capture = await tauriDesktopApi.startDictionaryTrainingSample()
    await tauriDesktopApi.finishDictionaryTrainingSample(String(capture))
    await tauriDesktopApi.cancelDictionaryTrainingSample(String(capture))
    await tauriDesktopApi.startCapture()
    await tauriDesktopApi.stopCapture('session')
    await tauriDesktopApi.cancelTranscription('session')
    await tauriDesktopApi.stopRecording('activation')
    await tauriDesktopApi.getRecordingLevel()
    await tauriDesktopApi.copyText('text')
    await tauriDesktopApi.quitApp()
    await tauriDesktopApi.removeStaleInstalls()
    const settings = tauriDesktopApi.getSettings()
    resolveLastChannel({ kind: 'ok', value: undefined })
    await settings
    await tauriDesktopApi.listModels()
    await tauriDesktopApi.listLanguages()
    const settingsWrite = tauriDesktopApi.setSettings(change)
    resolveLastChannel({ kind: 'ok', value: undefined })
    await settingsWrite
    await tauriDesktopApi.listGpuDevices()
    await tauriDesktopApi.listGpuDevices(true)
    const microphones = tauriDesktopApi.getMicrophones()
    resolveLastChannel({ kind: 'ok', value: undefined })
    await microphones
    const microphoneWrite = tauriDesktopApi.setMicrophone('device')
    resolveLastChannel({ kind: 'ok', value: undefined })
    await microphoneWrite
    await tauriDesktopApi.testInputDevice('device')
    await tauriDesktopApi.testMicrophoneFallback()
    await tauriDesktopApi.getReadiness()
    await tauriDesktopApi.startSetup('whisper-small')
    await tauriDesktopApi.startSetup('whisper-small', true)
    await tauriDesktopApi.repairManaged('whisper-small')
    await tauriDesktopApi.verifyManaged('whisper-small')
    await tauriDesktopApi.removeManaged('whisper-small')
    await tauriDesktopApi.cancelSetup('operation')

    expect(invokeMock.mock.calls).toEqual([
      ['get_app_status'],
      ['get_shortcut_status'],
      ['retry_shortcut'],
      ['repair_legacy_shortcut'],
      ['get_history'],
      ['delete_history_item', { id: 'history-id' }],
      ['clear_history'],
      ['get_dictionary'],
      ['add_dictionary_entry', { spoken: 'spoken', written: 'written' }],
      ['add_dictionary_entries_batch', { written: 'written', spoken: ['first', 'second'] }],
      ['remove_dictionary_entry', { spoken: 'spoken', written: 'written' }],
      ['start_dictionary_training_sample'],
      ['finish_dictionary_training_sample', { captureId: 'undefined' }],
      ['cancel_dictionary_training_sample', { captureId: 'undefined' }],
      ['start_capture'],
      ['stop_capture', { sessionId: 'session' }],
      ['cancel_transcription', { sessionId: 'session' }],
      ['stop_recording', { activation: 'activation' }],
      ['get_recording_level'],
      ['copy_text', { text: 'text' }],
      ['quit_app'],
      ['remove_stale_installs'],
      ['get_settings', { reply: queuedReply('get_settings') }],
      ['list_models'],
      ['list_languages'],
      ['set_settings', { change, reply: queuedReply('set_settings') }],
      ['list_gpu_devices', { refresh: false }],
      ['list_gpu_devices', { refresh: true }],
      ['get_microphones', { reply: queuedReply('get_microphones') }],
      ['set_microphone', { id: 'device', reply: queuedReply('set_microphone') }],
      ['test_input_device', { id: 'device' }],
      ['test_microphone_fallback'],
      ['get_readiness'],
      ['start_setup', { plan: 'whisper-small', managedCopy: false }],
      ['start_setup', { plan: 'whisper-small', managedCopy: true }],
      ['repair_managed', { component: 'whisper-small' }],
      ['verify_managed', { component: 'whisper-small' }],
      ['remove_managed', { component: 'whisper-small' }],
      ['cancel_setup', { operation: 'operation' }],
    ])
    expect(invokeMock).toHaveBeenCalledWith('delete_history_item', { id: 'history-id' })
    expect(invokeMock).toHaveBeenCalledWith('set_settings', {
      change,
      reply: queuedReply('set_settings'),
    })
    expect(queuedReply('get_settings')).toBeInstanceOf(TestChannel)
    expect(queuedReply('get_microphones')).toBeInstanceOf(TestChannel)
    expect(invokeMock).toHaveBeenCalledWith('set_microphone', {
      id: 'device',
      reply: queuedReply('set_microphone'),
    })
  })

  it('resolves queued command promises from the channel reply', async () => {
    const pending = tauriDesktopApi.getSettings()
    expect(hasReply(queuedArgs('get_settings'))).toBe(true)

    resolveLastChannel({ kind: 'ok', value: { revision: 42 } })

    await expect(pending).resolves.toEqual({ revision: 42 })
  })

  it('rejects queued command promises from the channel reply', async () => {
    const pending = tauriDesktopApi.setSettings({ kind: 'hud', value: false })

    resolveLastChannel({ kind: 'err', error: 'write failed' })

    await expect(pending).rejects.toThrow('write failed')
  })

  it('subscribes to setup-event and unwraps its payload', async () => {
    const handler = vi.fn()
    const event: SetupEvent = { kind: 'finished', operationId: 'operation' }
    await tauriDesktopApi.onSetupEvent(handler)

    expect(listenMock).toHaveBeenCalledWith('setup-event', expect.any(Function))
    const registration = requireFixture(listenMock.mock.calls[0], 'setup-event registration')
    const listener = requireFixture(registration[1], 'setup-event listener')
    listener({ payload: event })
    expect(handler).toHaveBeenCalledWith(event)
  })

  it('subscribes to settings-event notifications', async () => {
    const handler = vi.fn()
    await tauriDesktopApi.onSettingsEvent(handler)

    expect(listenMock).toHaveBeenCalledWith('settings-event', expect.any(Function))
    const registration = requireFixture(listenMock.mock.calls[0], 'settings-event registration')
    const listener = requireFixture(registration[1], 'settings-event listener')
    listener({ payload: { kind: 'finished', operationId: 'unused' } })
    expect(handler).toHaveBeenCalledOnce()
  })

  it('initializes below the five millisecond budget', () => {
    const durations = Array.from({ length: 51 }, (_, index) => {
      const start = `desktop-api-start-${index}`
      const end = `desktop-api-end-${index}`
      performance.mark(start)
      createTauriDesktopApi()
      performance.mark(end)
      const duration = performance.measure(`desktop-api-${index}`, start, end).duration
      performance.clearMarks(start)
      performance.clearMarks(end)
      performance.clearMeasures(`desktop-api-${index}`)
      return duration
    }).sort((left, right) => left - right)

    expect(requireFixture(durations[25], 'median desktop API initialization duration')).toBeLessThan(5)
  })
})
