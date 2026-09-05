import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AppStatus,
  DictionaryBatchResult,
  DictionaryItem,
  DictionaryTrainingSample,
  GpuDevice,
  HistoryItem,
  LanguageOptions,
  LegacyShortcutSetup,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  ModelInventory,
  Readiness,
  RecordingSnapshot,
  SettingsChange,
  SettingsSnapshot,
  SetupEvent,
  ShortcutStatus,
} from '../generated/ipc'
import type { DesktopApi } from './DesktopApi'

export function createTauriDesktopApi(): DesktopApi {
  return {
    getAppStatus: () => invoke<AppStatus>('get_app_status'),
    getShortcutStatus: () => invoke<ShortcutStatus>('get_shortcut_status'),
    retryShortcut: () => invoke<ShortcutStatus>('retry_shortcut'),
    repairLegacyShortcut: () => invoke<LegacyShortcutSetup>('repair_legacy_shortcut'),
    getHistory: () => invoke<HistoryItem[]>('get_history'),
    deleteHistoryItem: (id) => invoke<boolean>('delete_history_item', { id }),
    clearHistory: () => invoke<number>('clear_history'),
    getDictionary: () => invoke<DictionaryItem[]>('get_dictionary'),
    addDictionaryEntry: (spoken, written) =>
      invoke<DictionaryItem>('add_dictionary_entry', { spoken, written }),
    addDictionaryEntriesBatch: (written, spoken) =>
      invoke<DictionaryBatchResult>('add_dictionary_entries_batch', { written, spoken }),
    removeDictionaryEntry: (spoken, written) =>
      invoke<boolean>('remove_dictionary_entry', { spoken, written }),
    startDictionaryTrainingSample: () =>
      invoke<string>('start_dictionary_training_sample'),
    finishDictionaryTrainingSample: (captureId) =>
      invoke<DictionaryTrainingSample>('finish_dictionary_training_sample', { captureId }),
    cancelDictionaryTrainingSample: (captureId) =>
      invoke<boolean>('cancel_dictionary_training_sample', { captureId }),
    startCapture: () => invoke<RecordingSnapshot>('start_capture'),
    stopCapture: (sessionId) => invoke<RecordingSnapshot>('stop_capture', { sessionId }),
    cancelTranscription: (sessionId) => invoke<RecordingSnapshot>('cancel_transcription', { sessionId }),
    stopRecording: (activation) => invoke<boolean>('stop_recording', { activation }),
    getRecordingLevel: () => invoke<number>('get_recording_level'),
    copyText: (text) => invoke<void>('copy_text', { text }),
    quitApp: () => invoke<void>('quit_app'),
    removeStaleInstalls: () => invoke<string[]>('remove_stale_installs'),
    getSettings: () => invoke<SettingsSnapshot>('get_settings'),
    listModels: () => invoke<ModelInventory>('list_models'),
    listLanguages: () => invoke<LanguageOptions>('list_languages'),
    setSettings: (change: SettingsChange) =>
      invoke<SettingsSnapshot>('set_settings', { change }),
    listGpuDevices: (refresh = false) => invoke<GpuDevice[]>('list_gpu_devices', { refresh }),
    getMicrophones: () => invoke<MicrophoneSnapshot>('get_microphones'),
    setMicrophone: (id) => invoke<MicrophoneSnapshot>('set_microphone', { id }),
    testInputDevice: (id) => invoke<MicrophoneTestResult>('test_input_device', { id }),
    testMicrophoneFallback: () => invoke<MicrophoneTestResult>('test_microphone_fallback'),
    getReadiness: () => invoke<Readiness>('get_readiness'),
    startSetup: (plan, managedCopy = false) =>
      invoke<string>('start_setup', { plan, managedCopy }),
    repairManaged: (component) => invoke<string>('repair_managed', { component }),
    verifyManaged: (component) => invoke<string>('verify_managed', { component }),
    removeManaged: (component) => invoke<string>('remove_managed', { component }),
    cancelSetup: (operation) => invoke<boolean>('cancel_setup', { operation }),
    onSetupEvent: (handler) =>
      listen<SetupEvent>('setup-event', (event) => handler(event.payload)),
    onSettingsEvent: (handler) =>
      listen('settings-event', () => handler()),
  }
}

export const tauriDesktopApi = createTauriDesktopApi()
