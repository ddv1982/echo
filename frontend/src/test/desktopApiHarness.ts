import { vi } from 'vitest'
import type { DesktopApi } from '../api/DesktopApi'

export function createDesktopApiMocks(actual: DesktopApi): DesktopApi {
  return {
    getAppStatus: vi.fn(actual.getAppStatus),
    getShortcutStatus: vi.fn(actual.getShortcutStatus),
    retryShortcut: vi.fn(actual.retryShortcut),
    repairLegacyShortcut: vi.fn(actual.repairLegacyShortcut),
    getHistory: vi.fn(actual.getHistory),
    deleteHistoryItem: vi.fn(actual.deleteHistoryItem),
    clearHistory: vi.fn(actual.clearHistory),
    getDictionary: vi.fn(actual.getDictionary),
    addDictionaryEntry: vi.fn(actual.addDictionaryEntry),
    addDictionaryEntriesBatch: vi.fn(actual.addDictionaryEntriesBatch),
    removeDictionaryEntry: vi.fn(actual.removeDictionaryEntry),
    startDictionaryTrainingSample: vi.fn(actual.startDictionaryTrainingSample),
    finishDictionaryTrainingSample: vi.fn(actual.finishDictionaryTrainingSample),
    cancelDictionaryTrainingSample: vi.fn(actual.cancelDictionaryTrainingSample),
    startCapture: vi.fn(actual.startCapture),
    stopCapture: vi.fn(actual.stopCapture),
    cancelTranscription: vi.fn(actual.cancelTranscription),
    stopRecording: vi.fn(actual.stopRecording),
    getRecordingLevel: vi.fn(actual.getRecordingLevel),
    copyText: vi.fn(actual.copyText),
    quitApp: vi.fn(actual.quitApp),
    removeStaleInstalls: vi.fn(actual.removeStaleInstalls),
    getSettings: vi.fn(actual.getSettings),
    listModels: vi.fn(actual.listModels),
    listLanguages: vi.fn(actual.listLanguages),
    setSettings: vi.fn(actual.setSettings),
    listGpuDevices: vi.fn(actual.listGpuDevices),
    getMicrophones: vi.fn(actual.getMicrophones),
    setMicrophone: vi.fn(actual.setMicrophone),
    testInputDevice: vi.fn(actual.testInputDevice),
    testMicrophoneFallback: vi.fn(actual.testMicrophoneFallback),
    getReadiness: vi.fn(actual.getReadiness),
    startSetup: vi.fn(actual.startSetup),
    repairManaged: vi.fn(actual.repairManaged),
    verifyManaged: vi.fn(actual.verifyManaged),
    removeManaged: vi.fn(actual.removeManaged),
    cancelSetup: vi.fn(actual.cancelSetup),
    onSetupEvent: vi.fn(actual.onSetupEvent),
    onSettingsEvent: vi.fn(actual.onSettingsEvent),
  }
}

export function resetDesktopApiMocks(mocks: DesktopApi, actual: DesktopApi): void {
  vi.mocked(mocks.getAppStatus).mockReset().mockImplementation(actual.getAppStatus)
  vi.mocked(mocks.getShortcutStatus).mockReset().mockImplementation(actual.getShortcutStatus)
  vi.mocked(mocks.retryShortcut).mockReset().mockImplementation(actual.retryShortcut)
  vi.mocked(mocks.repairLegacyShortcut).mockReset().mockImplementation(actual.repairLegacyShortcut)
  vi.mocked(mocks.getHistory).mockReset().mockImplementation(actual.getHistory)
  vi.mocked(mocks.deleteHistoryItem).mockReset().mockResolvedValue(true)
  vi.mocked(mocks.clearHistory).mockReset().mockResolvedValue(3)
  vi.mocked(mocks.getDictionary).mockReset().mockImplementation(actual.getDictionary)
  vi.mocked(mocks.addDictionaryEntry).mockReset().mockImplementation(actual.addDictionaryEntry)
  vi.mocked(mocks.addDictionaryEntriesBatch).mockReset().mockImplementation(actual.addDictionaryEntriesBatch)
  vi.mocked(mocks.removeDictionaryEntry).mockReset().mockImplementation(actual.removeDictionaryEntry)
  vi.mocked(mocks.startDictionaryTrainingSample).mockReset().mockImplementation(actual.startDictionaryTrainingSample)
  vi.mocked(mocks.finishDictionaryTrainingSample).mockReset().mockImplementation(actual.finishDictionaryTrainingSample)
  vi.mocked(mocks.cancelDictionaryTrainingSample).mockReset().mockImplementation(actual.cancelDictionaryTrainingSample)
  vi.mocked(mocks.startCapture).mockReset().mockImplementation(actual.startCapture)
  vi.mocked(mocks.stopCapture).mockReset().mockImplementation(actual.stopCapture)
  vi.mocked(mocks.cancelTranscription).mockReset().mockImplementation(actual.cancelTranscription)
  vi.mocked(mocks.stopRecording).mockReset().mockImplementation(actual.stopRecording)
  vi.mocked(mocks.getRecordingLevel).mockReset().mockImplementation(actual.getRecordingLevel)
  vi.mocked(mocks.copyText).mockReset().mockImplementation(actual.copyText)
  vi.mocked(mocks.quitApp).mockReset().mockImplementation(actual.quitApp)
  vi.mocked(mocks.removeStaleInstalls).mockReset().mockImplementation(actual.removeStaleInstalls)
  vi.mocked(mocks.getSettings).mockReset().mockImplementation(actual.getSettings)
  vi.mocked(mocks.listModels).mockReset().mockImplementation(actual.listModels)
  vi.mocked(mocks.listLanguages).mockReset().mockImplementation(actual.listLanguages)
  vi.mocked(mocks.setSettings).mockReset().mockImplementation(actual.setSettings)
  vi.mocked(mocks.listGpuDevices).mockReset().mockImplementation(actual.listGpuDevices)
  vi.mocked(mocks.getMicrophones).mockReset().mockImplementation(actual.getMicrophones)
  vi.mocked(mocks.setMicrophone).mockReset().mockImplementation(actual.setMicrophone)
  vi.mocked(mocks.testInputDevice).mockReset().mockImplementation(actual.testInputDevice)
  vi.mocked(mocks.testMicrophoneFallback).mockReset().mockImplementation(actual.testMicrophoneFallback)
  vi.mocked(mocks.getReadiness).mockReset().mockImplementation(actual.getReadiness)
  vi.mocked(mocks.startSetup).mockReset().mockImplementation(actual.startSetup)
  vi.mocked(mocks.repairManaged).mockReset().mockImplementation(actual.repairManaged)
  vi.mocked(mocks.verifyManaged).mockReset().mockImplementation(actual.verifyManaged)
  vi.mocked(mocks.removeManaged).mockReset().mockImplementation(actual.removeManaged)
  vi.mocked(mocks.cancelSetup).mockReset().mockImplementation(actual.cancelSetup)
  vi.mocked(mocks.onSetupEvent).mockReset().mockImplementation(actual.onSetupEvent)
  vi.mocked(mocks.onSettingsEvent).mockReset().mockImplementation(actual.onSettingsEvent)
}

export function deferred<T>() {
  let resolvePromise: ((value: T | PromiseLike<T>) => void) | undefined
  let rejectPromise: ((reason?: unknown) => void) | undefined
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve
    rejectPromise = reject
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!resolvePromise) throw new Error('deferred promise is not initialized')
      resolvePromise(value)
    },
    reject(reason?: unknown) {
      if (!rejectPromise) throw new Error('deferred promise is not initialized')
      rejectPromise(reason)
    },
  }
}
