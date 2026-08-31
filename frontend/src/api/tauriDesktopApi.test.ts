import type { SettingsChange, SetupEvent } from '../generated/ipc'
import { createTauriDesktopApi, tauriDesktopApi } from './tauriDesktopApi'

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn((command: string, commandArguments?: unknown) => {
    void command
    void commandArguments
    return Promise.resolve()
  }),
  listenMock: vi.fn((event: string, handler: (event: { payload: SetupEvent }) => void) => {
    void event
    void handler
    return Promise.resolve(() => undefined)
  }),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

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
    await tauriDesktopApi.getDictionary()
    await tauriDesktopApi.addDictionaryEntry('spoken', 'written')
    await tauriDesktopApi.removeDictionaryEntry('spoken', 'written')
    await tauriDesktopApi.toggleRecording()
    await tauriDesktopApi.stopRecording('activation')
    await tauriDesktopApi.getRecordingLevel()
    await tauriDesktopApi.copyText('text')
    await tauriDesktopApi.removeStaleInstalls()
    await tauriDesktopApi.getSettings()
    await tauriDesktopApi.listModels()
    await tauriDesktopApi.listLanguages()
    await tauriDesktopApi.setSettings(change)
    await tauriDesktopApi.listGpuDevices()
    await tauriDesktopApi.listGpuDevices(true)
    await tauriDesktopApi.getMicrophones()
    await tauriDesktopApi.setMicrophone('device')
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
      ['get_dictionary'],
      ['add_dictionary_entry', { spoken: 'spoken', written: 'written' }],
      ['remove_dictionary_entry', { spoken: 'spoken', written: 'written' }],
      ['toggle_recording'],
      ['stop_recording', { activation: 'activation' }],
      ['get_recording_level'],
      ['copy_text', { text: 'text' }],
      ['remove_stale_installs'],
      ['get_settings'],
      ['list_models'],
      ['list_languages'],
      ['set_settings', { change }],
      ['list_gpu_devices', { refresh: false }],
      ['list_gpu_devices', { refresh: true }],
      ['get_microphones'],
      ['set_microphone', { id: 'device' }],
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
  })

  it('subscribes to setup-event and unwraps its payload', async () => {
    const handler = vi.fn()
    const event: SetupEvent = { kind: 'finished', operationId: 'operation' }
    await tauriDesktopApi.onSetupEvent(handler)

    expect(listenMock).toHaveBeenCalledWith('setup-event', expect.any(Function))
    const listener = listenMock.mock.calls[0][1]
    listener({ payload: event })
    expect(handler).toHaveBeenCalledWith(event)
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

    expect(durations[25]).toBeLessThan(5)
  })
})
