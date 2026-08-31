import type {
  AppStatus,
  ComponentId,
  DictionaryItem,
  GpuDevice,
  HistoryItem,
  LanguageOptions,
  LegacyShortcutSetup,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  ModelInventory,
  Readiness,
  SettingsChange,
  SettingsSnapshot,
  SetupEvent,
  SetupPlanId,
  ShortcutStatus,
} from '../generated/ipc'

export interface DesktopApi {
  getAppStatus(): Promise<AppStatus>
  getShortcutStatus(): Promise<ShortcutStatus>
  retryShortcut(): Promise<ShortcutStatus>
  repairLegacyShortcut(): Promise<LegacyShortcutSetup>
  getHistory(): Promise<HistoryItem[]>
  getDictionary(): Promise<DictionaryItem[]>
  addDictionaryEntry(spoken: string, written: string): Promise<DictionaryItem>
  removeDictionaryEntry(spoken: string, written: string): Promise<boolean>
  toggleRecording(): Promise<void>
  stopRecording(activation: string): Promise<boolean>
  getRecordingLevel(): Promise<number>
  copyText(text: string): Promise<void>
  removeStaleInstalls(): Promise<string[]>
  getSettings(): Promise<SettingsSnapshot>
  listModels(): Promise<ModelInventory>
  listLanguages(): Promise<LanguageOptions>
  setSettings(change: SettingsChange): Promise<SettingsSnapshot>
  listGpuDevices(refresh?: boolean): Promise<GpuDevice[]>
  getMicrophones(): Promise<MicrophoneSnapshot>
  setMicrophone(id: string | null): Promise<MicrophoneSnapshot>
  testInputDevice(id: string | null): Promise<MicrophoneTestResult>
  testMicrophoneFallback(): Promise<MicrophoneTestResult>
  getReadiness(): Promise<Readiness>
  startSetup(plan: SetupPlanId, managedCopy?: boolean): Promise<string>
  repairManaged(component: ComponentId): Promise<string>
  verifyManaged(component: ComponentId): Promise<string>
  removeManaged(component: ComponentId): Promise<string>
  cancelSetup(operation: string): Promise<boolean>
  onSetupEvent(handler: (event: SetupEvent) => void): Promise<() => void>
}
