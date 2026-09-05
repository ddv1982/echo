import type {
  AppStatus,
  RecordingSnapshot,
  ComponentId,
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
  deleteHistoryItem(id: string): Promise<boolean>
  clearHistory(): Promise<number>
  getDictionary(): Promise<DictionaryItem[]>
  addDictionaryEntry(spoken: string, written: string): Promise<DictionaryItem>
  addDictionaryEntriesBatch(written: string, spoken: string[]): Promise<DictionaryBatchResult>
  removeDictionaryEntry(spoken: string, written: string): Promise<boolean>
  startDictionaryTrainingSample(): Promise<string>
  finishDictionaryTrainingSample(captureId: string): Promise<DictionaryTrainingSample>
  cancelDictionaryTrainingSample(captureId: string): Promise<boolean>
  startCapture(): Promise<RecordingSnapshot>
  stopCapture(sessionId: string): Promise<RecordingSnapshot>
  cancelTranscription(sessionId: string): Promise<RecordingSnapshot>
  stopRecording(activation: string): Promise<boolean>
  getRecordingLevel(): Promise<number>
  copyText(text: string): Promise<void>
  quitApp(): Promise<void>
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
  onSettingsEvent(handler: () => void): Promise<() => void>
}
