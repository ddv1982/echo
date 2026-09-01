import type { DesktopApi } from './api/DesktopApi'

let desktopApi: DesktopApi | undefined

export function configureDesktopApi(api: DesktopApi): void {
  desktopApi = api
}

function api(): DesktopApi {
  if (!desktopApi) throw new Error('desktop adapter is not configured')
  return desktopApi
}

export const getAppStatus: DesktopApi['getAppStatus'] = () => api().getAppStatus()
export const getShortcutStatus: DesktopApi['getShortcutStatus'] = () => api().getShortcutStatus()
export const retryShortcut: DesktopApi['retryShortcut'] = () => api().retryShortcut()
export const repairLegacyShortcut: DesktopApi['repairLegacyShortcut'] = () => api().repairLegacyShortcut()
export const getHistory: DesktopApi['getHistory'] = () => api().getHistory()
export const deleteHistoryItem: DesktopApi['deleteHistoryItem'] = (id) => api().deleteHistoryItem(id)
export const clearHistory: DesktopApi['clearHistory'] = () => api().clearHistory()
export const getDictionary: DesktopApi['getDictionary'] = () => api().getDictionary()
export const addDictionaryEntry: DesktopApi['addDictionaryEntry'] = (spoken, written) =>
  api().addDictionaryEntry(spoken, written)
export const addDictionaryEntriesBatch: DesktopApi['addDictionaryEntriesBatch'] = (written, spoken) =>
  api().addDictionaryEntriesBatch(written, spoken)
export const removeDictionaryEntry: DesktopApi['removeDictionaryEntry'] = (spoken, written) =>
  api().removeDictionaryEntry(spoken, written)
export const startDictionaryTrainingSample: DesktopApi['startDictionaryTrainingSample'] = () =>
  api().startDictionaryTrainingSample()
export const finishDictionaryTrainingSample: DesktopApi['finishDictionaryTrainingSample'] = (captureId) =>
  api().finishDictionaryTrainingSample(captureId)
export const cancelDictionaryTrainingSample: DesktopApi['cancelDictionaryTrainingSample'] = (captureId) =>
  api().cancelDictionaryTrainingSample(captureId)
export const toggleRecording: DesktopApi['toggleRecording'] = () => api().toggleRecording()
export const stopRecording: DesktopApi['stopRecording'] = (activation) => api().stopRecording(activation)
export const getRecordingLevel: DesktopApi['getRecordingLevel'] = () => api().getRecordingLevel()
export const copyText: DesktopApi['copyText'] = (text) => api().copyText(text)
export const quitApp: DesktopApi['quitApp'] = () => api().quitApp()
export const removeStaleInstalls: DesktopApi['removeStaleInstalls'] = () => api().removeStaleInstalls()
export const getSettings: DesktopApi['getSettings'] = () => api().getSettings()
export const listModels: DesktopApi['listModels'] = () => api().listModels()
export const listLanguages: DesktopApi['listLanguages'] = () => api().listLanguages()
export const setSettings: DesktopApi['setSettings'] = (settings) => api().setSettings(settings)
export const listGpuDevices: DesktopApi['listGpuDevices'] = (refresh = false) => api().listGpuDevices(refresh)
export const getMicrophones: DesktopApi['getMicrophones'] = () => api().getMicrophones()
export const setMicrophone: DesktopApi['setMicrophone'] = (id) =>
  api().setMicrophone(id)
export const testInputDevice: DesktopApi['testInputDevice'] = (id) =>
  api().testInputDevice(id)
export const testMicrophoneFallback: DesktopApi['testMicrophoneFallback'] = () =>
  api().testMicrophoneFallback()
export const getReadiness: DesktopApi['getReadiness'] = () => api().getReadiness()
export const startSetup: DesktopApi['startSetup'] = (plan, managedCopy = false) =>
  api().startSetup(plan, managedCopy)
export const repairManaged: DesktopApi['repairManaged'] = (component) =>
  api().repairManaged(component)
export const verifyManaged: DesktopApi['verifyManaged'] = (component) =>
  api().verifyManaged(component)
export const removeManaged: DesktopApi['removeManaged'] = (component) =>
  api().removeManaged(component)
export const cancelSetup: DesktopApi['cancelSetup'] = (operation) => api().cancelSetup(operation)
export const onSetupEvent: DesktopApi['onSetupEvent'] = (handler) =>
  api().onSetupEvent(handler)
