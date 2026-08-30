import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AppStatus,
  DictionaryItem,
  GpuDevice,
  HistoryItem,
  LanguageOptions,
  LegacyShortcutSetup,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  ModelInventory,
  Readiness,
  Settings,
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
    getDictionary: () => invoke<DictionaryItem[]>('get_dictionary'),
    addDictionaryEntry: (spoken, written) =>
      invoke<DictionaryItem>('add_dictionary_entry', { spoken, written }),
    removeDictionaryEntry: (spoken, written) =>
      invoke<boolean>('remove_dictionary_entry', { spoken, written }),
    toggleRecording: () => invoke<void>('toggle_recording'),
    stopRecording: (activation) => invoke<boolean>('stop_recording', { activation }),
    getRecordingLevel: () => invoke<number>('get_recording_level'),
    copyText: (text) => invoke<void>('copy_text', { text }),
    removeStaleInstalls: () => invoke<string[]>('remove_stale_installs'),
    getSettings: () => invoke<Settings>('get_settings'),
    listModels: () => invoke<ModelInventory>('list_models'),
    listLanguages: () => invoke<LanguageOptions>('list_languages'),
    setSettings: (settings) => invoke<Settings>('set_settings', { settings }),
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
  }
}

export const tauriDesktopApi = createTauriDesktopApi()
