import { useCallback, useEffect, useRef, useState } from 'react'

import { messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { applySetupProgress } from '../setup'
import {
  getMicrophones,
  getReadiness,
  getSettings,
  listGpuDevices,
  listLanguages,
  listModels,
  onSetupEvent,
  repairLegacyShortcut,
  repairManaged,
  retryShortcut,
  setMicrophone,
  setSettings,
  testInputDevice,
  testMicrophoneFallback,
} from '../tauri'
import type {
  GpuDevice,
  LanguageOptions,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  ModelInventory,
  Readiness,
  Settings as AppSettings,
  SetupEvent,
} from '../generated/ipc'

interface UseSettingsControllerArgs {
  onStatusChange: () => Promise<void>
  onError: (message: string) => void
}

export function useSettingsController({
  onStatusChange,
  onError,
}: UseSettingsControllerArgs) {
  const [settings, setLocalSettings] = useState<AppSettings | null>(null)
  const settingsRef = useRef<AppSettings | null>(null)
  const writeChainRef = useRef(Promise.resolve())
  const settingsMutationVersion = useRef(0)
  const active = useRef(true)
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [inventory, setInventory] = useState<ModelInventory | null>(null)
  const [languages, setLanguages] = useState<LanguageOptions | null>(null)
  const [readiness, setReadiness] = useState<Readiness | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const [repairingLegacyShortcut, setRepairingLegacyShortcut] = useState(false)
  const [settingsWritePending, setSettingsWritePending] = useState(false)
  const [gpuDevices, setGpuDevices] = useState<GpuDevice[]>([])
  const micTestVersion = useRef(0)

  const reportSettingsError = useCallback((reason: unknown) => {
    if (active.current) onError(messageFrom(reason))
  }, [onError])

  useEffect(() => {
    let current = true
    active.current = true
    void getSettings()
      .then((next) => {
        if (current && active.current) {
          settingsRef.current = next
          setLocalSettings(next)
        }
      })
      .catch((reason: unknown) => {
        if (current && active.current) reportSettingsError(reason)
      })
    void listModels().then((next) => {
      if (current && active.current) setInventory(next)
    }).catch((reason: unknown) => {
      if (current && active.current) reportSettingsError(reason)
    })
    void listLanguages().then((next) => {
      if (current && active.current) setLanguages(next)
    }).catch((reason: unknown) => {
      if (current && active.current) reportSettingsError(reason)
    })
    void getReadiness().then((next) => {
      if (current && active.current) setReadiness(next)
    }).catch((reason: unknown) => {
      if (current && active.current) reportSettingsError(reason)
    })
    return () => {
      current = false
      active.current = false
      micTestVersion.current += 1
    }
  }, [reportSettingsError])

  const wantsGpu = settings?.whisperAcceleration.effective === 'gpu'
  const gpuPrerequisite = (['whisper-runtime', 'whisper-vulkan-runtime'] as const)
    .map((id) => readiness?.components.find((component) => component.id === id) ?? null)
    .find((component) => component != null && component.managed.kind !== 'ready') ?? null
  const gpuRuntimeReady = readiness != null && gpuPrerequisite == null

  useEffect(() => {
    if (!wantsGpu || !gpuRuntimeReady) return
    let active = true
    void listGpuDevices(true)
      .then((next) => {
        if (active) setGpuDevices(next)
      })
      .catch((reason: unknown) => {
        if (active) reportSettingsError(reason)
      })
    return () => {
      active = false
    }
  }, [wantsGpu, gpuRuntimeReady, reportSettingsError])

  const refreshMicrophones = useSerialPoll({
    request: getMicrophones,
    onResult: setMicrophones,
    onError: reportSettingsError,
    intervalMs: 3_000,
  })

  useEffect(() => {
    const refreshOnFocus = () => void refreshMicrophones()
    window.addEventListener('focus', refreshOnFocus)
    return () => {
      window.removeEventListener('focus', refreshOnFocus)
    }
  }, [refreshMicrophones])

  const handleSettingsSetupEvent = useCallback((event: SetupEvent) => {
    if (event.kind === 'progress') {
      setReadiness((current) => current && applySetupProgress(current, event))
    }
    if (event.kind === 'failed') onError(event.error)
  }, [onError])
  const getSettingsSetupRefresh = useCallback((event: SetupEvent) => {
    if (event.kind === 'progress') return null
    const version = settingsMutationVersion.current
    return () => Promise.all([getReadiness(), listModels(), getSettings(), listLanguages()])
      .then(([nextReadiness, nextInventory, nextSettings, nextLanguages]) => () => {
        setReadiness(nextReadiness)
        setInventory(nextInventory)
        if (settingsMutationVersion.current === version) {
          settingsRef.current = nextSettings
          setLocalSettings(nextSettings)
          setLanguages(nextLanguages)
        }
        void onStatusChange()
      })
  }, [onStatusChange])
  useAsyncSubscription({
    subscribe: onSetupEvent,
    onEvent: handleSettingsSetupEvent,
    getRefresh: getSettingsSetupRefresh,
    onError: reportSettingsError,
  })

  useEffect(() => {
    settingsRef.current = settings
  }, [settings])

  const commit = useCallback(async (next: AppSettings) => {
    try {
      const written = await setSettings(next)
      if (!active.current) return
      settingsRef.current = written
      setLocalSettings(written)
      setLanguages(null)
      const [statusResult, languageResult] = await Promise.allSettled([
        onStatusChange(),
        listLanguages(),
      ])
      if (!active.current) return
      if (statusResult.status === 'rejected') reportSettingsError(statusResult.reason)
      if (languageResult.status === 'fulfilled') {
        setLanguages(languageResult.value)
      } else {
        reportSettingsError(languageResult.reason)
      }
    } catch (reason) {
      reportSettingsError(reason)
    }
  }, [onStatusChange, reportSettingsError])

  const updateSettings = useCallback(async (update: (current: AppSettings) => AppSettings) => {
    setSettingsWritePending(true)
    settingsMutationVersion.current += 1
    const queued = writeChainRef.current.then(async () => {
      const current = settingsRef.current
      if (!current) return
      await commit(update(current))
    })
    writeChainRef.current = queued
    try {
      await queued
    } finally {
      if (active.current && writeChainRef.current === queued) setSettingsWritePending(false)
    }
  }, [commit])

  const patch = useCallback(async <K extends keyof AppSettings>(key: K, value: AppSettings[K]['value']) => {
    await updateSettings((current) => ({ ...current, [key]: { ...current[key], value } }))
  }, [updateSettings])

  const selectEngine = useCallback(async (engine: string) => {
    await updateSettings((current) => {
      if (engine !== 'parakeet') {
        return { ...current, engine: { ...current.engine, value: engine } }
      }
      return {
        ...current,
        engine: { ...current.engine, value: engine },
        whisperModel: { ...current.whisperModel, value: null },
      }
    })
  }, [updateSettings])

  const repairLegacy = useCallback(async () => {
    setRepairingLegacyShortcut(true)
    try {
      await repairLegacyShortcut()
      if (!active.current) return
      await onStatusChange()
    } catch (reason) {
      reportSettingsError(reason)
    } finally {
      if (active.current) setRepairingLegacyShortcut(false)
    }
  }, [onStatusChange, reportSettingsError])

  const retryShortcutStatus = useCallback(async () => {
    try {
      await retryShortcut()
      if (!active.current) return
      await onStatusChange()
    } catch (reason) {
      reportSettingsError(reason)
    }
  }, [onStatusChange, reportSettingsError])

  const refreshReadiness = useCallback(() => {
    void getReadiness().then((next) => {
      if (active.current) setReadiness(next)
    }).catch(reportSettingsError)
  }, [reportSettingsError])

  const installGpuPrerequisite = useCallback(() => {
    if (gpuPrerequisite == null) return
    void repairManaged(gpuPrerequisite.id)
      .then(() => getReadiness())
      .then((next) => {
        if (active.current) setReadiness(next)
      })
      .catch(reportSettingsError)
  }, [gpuPrerequisite, reportSettingsError])

  const refreshGpuDevices = useCallback(() => {
    void listGpuDevices(true).then((next) => {
      if (active.current) setGpuDevices(next)
    }).catch(reportSettingsError)
  }, [reportSettingsError])

  const selectMicrophone = useCallback((id: string | null) => {
    micTestVersion.current += 1
    setMicTest(null)
    void setMicrophone(id)
      .then((next) => {
        if (!active.current) return null
        setMicrophones(next)
        return onStatusChange()
      })
      .catch(reportSettingsError)
  }, [onStatusChange, reportSettingsError])

  const testMicrophone = useCallback((id: string | null, fallback: boolean) => {
    const version = ++micTestVersion.current
    setTestingMic(true)
    const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
    void run
      .then((result) => {
        if (micTestVersion.current === version) setMicTest(result)
      })
      .catch((reason: unknown) => {
        if (micTestVersion.current === version) reportSettingsError(reason)
      })
      .finally(() => {
        if (micTestVersion.current === version) setTestingMic(false)
      })
  }, [reportSettingsError])

  const parakeetRuns = languages?.mode === 'parakeet'
  const whisperRuns =
    languages != null && settings?.engine.effective !== 'fake' && !parakeetRuns

  return {
    settings,
    microphones,
    inventory,
    languages,
    readiness,
    micTest,
    testingMic,
    repairingLegacyShortcut,
    settingsWritePending,
    gpuDevices,
    gpuPrerequisite,
    parakeetRuns,
    whisperRuns,
    patch,
    selectEngine,
    repairLegacy,
    retryShortcutStatus,
    refreshMicrophones,
    refreshReadiness,
    installGpuPrerequisite,
    refreshGpuDevices,
    selectMicrophone,
    testMicrophone,
  }
}
