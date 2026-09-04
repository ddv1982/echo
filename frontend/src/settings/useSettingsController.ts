import { useCallback, useEffect, useRef, useState } from 'react'

import { messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { applySetupProgress, classifySetupEvent } from '../setup'
import {
  getMicrophones,
  getSettings,
  listGpuDevices,
  onSettingsEvent,
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
  MicrophoneSnapshot,
  MicrophoneTestResult,
  SettingsChange,
  SettingsSnapshot,
  SetupEvent,
} from '../generated/ipc'

interface UseSettingsControllerArgs {
  onStatusChange: () => Promise<void>
  onError: (message: string) => void
}

interface SettingsReadResult {
  snapshot: SettingsSnapshot
  mutationVersion: number
  readVersion: number
}

export function useSettingsController({
  onStatusChange,
  onError,
}: UseSettingsControllerArgs) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null)
  const writeChainRef = useRef(Promise.resolve())
  const settingsMutationVersion = useRef(0)
  const settingsReadVersion = useRef(0)
  const active = useRef(true)
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const [repairingLegacyShortcut, setRepairingLegacyShortcut] = useState(false)
  const [settingsWritePending, setSettingsWritePending] = useState(false)
  const [gpuDevices, setGpuDevices] = useState<GpuDevice[]>([])
  const micTestVersion = useRef(0)

  const reportSettingsError = useCallback((reason: unknown) => {
    if (active.current) onError(messageFrom(reason))
  }, [onError])

  const loadSettingsSnapshot = useCallback(async (): Promise<SettingsReadResult | null> => {
    const readVersion = ++settingsReadVersion.current
    const mutationVersion = settingsMutationVersion.current
    const queuedWrites = writeChainRef.current
    await queuedWrites
    if (!active.current || settingsMutationVersion.current !== mutationVersion) return null
    return { snapshot: await getSettings(), mutationVersion, readVersion }
  }, [])

  const applySettingsSnapshot = useCallback((result: SettingsReadResult | null) => {
    if (
      result != null &&
      active.current &&
      settingsMutationVersion.current === result.mutationVersion &&
      settingsReadVersion.current === result.readVersion
    ) {
      setSnapshot(result.snapshot)
    }
  }, [])

  useEffect(() => {
    let current = true
    active.current = true
    void loadSettingsSnapshot()
      .then((result) => {
        if (current) applySettingsSnapshot(result)
      })
      .catch((reason: unknown) => {
        if (current && active.current) reportSettingsError(reason)
      })
    return () => {
      current = false
      active.current = false
      micTestVersion.current += 1
    }
  }, [applySettingsSnapshot, loadSettingsSnapshot, reportSettingsError])

  const settings = snapshot?.preferences ?? null
  const inventory = snapshot?.transcription.models ?? null
  const languages = snapshot?.transcription.languages ?? null
  const readiness = snapshot?.readiness ?? null
  const nextRun = snapshot?.transcription.nextRun ?? null
  const whisper = snapshot?.transcription.whisper ?? null
  const lastUsed = snapshot?.transcription.lastUsed ?? null
  const wantsGpu =
    whisper?.kind === 'applicable' && settings?.whisperAcceleration.effective === 'gpu'
  const gpuPrerequisite =
    whisper?.kind === 'applicable' &&
    (whisper.gpu.kind === 'needs-install' || whisper.gpu.kind === 'unsupported')
      ? whisper.gpu.component
      : null
  const gpuRuntimeReady = whisper?.kind === 'applicable' && whisper.gpu.kind === 'ready'

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
    const classified = classifySetupEvent(event)
    if (classified.kind === 'incremental') {
      setSnapshot((current) => current && {
        ...current,
        readiness: applySetupProgress(current.readiness, classified.event),
      })
    }
    if (classified.kind === 'terminal' && classified.error != null) {
      reportSettingsError(classified.error)
    }
  }, [reportSettingsError])
  const getSettingsSetupRefresh = useCallback((event: SetupEvent) => {
    if (classifySetupEvent(event).kind === 'incremental') return null
    return () => loadSettingsSnapshot().then((result) => async () => {
      applySettingsSnapshot(result)
      await onStatusChange()
    })
  }, [applySettingsSnapshot, loadSettingsSnapshot, onStatusChange])
  useAsyncSubscription({
    subscribe: onSetupEvent,
    onEvent: handleSettingsSetupEvent,
    getRefresh: getSettingsSetupRefresh,
    onError: reportSettingsError,
  })
  const getSettingsEventRefresh = useCallback(() => () =>
    loadSettingsSnapshot().then((result) => async () => {
      applySettingsSnapshot(result)
      await onStatusChange()
    }), [applySettingsSnapshot, loadSettingsSnapshot, onStatusChange])
  const handleSettingsEvent = useCallback(() => undefined, [])
  useAsyncSubscription<void>({
    subscribe: onSettingsEvent,
    onEvent: handleSettingsEvent,
    getRefresh: getSettingsEventRefresh,
    onError: reportSettingsError,
  })

  const commit = useCallback(async (change: SettingsChange) => {
    try {
      const written = await setSettings(change)
      if (!active.current) return
      setSnapshot(written)
      await onStatusChange()
    } catch (reason) {
      reportSettingsError(reason)
      throw reason
    }
  }, [onStatusChange, reportSettingsError])

  const updateSettings = useCallback(async (change: SettingsChange) => {
    setSettingsWritePending(true)
    settingsMutationVersion.current += 1
    const queued = writeChainRef.current.then(() => commit(change))
    const continuing = queued.catch(() => undefined)
    writeChainRef.current = continuing
    try {
      await queued
    } catch {
      const result = await loadSettingsSnapshot().catch((reason: unknown) => {
        reportSettingsError(reason)
        return null
      })
      applySettingsSnapshot(result)
    } finally {
      if (active.current && writeChainRef.current === continuing) setSettingsWritePending(false)
    }
  }, [applySettingsSnapshot, commit, loadSettingsSnapshot, reportSettingsError])

  const selectEngine = useCallback(async (engine: string) => {
    await updateSettings({ kind: 'engine', value: engine })
  }, [updateSettings])

  const enableWhisperGpu = useCallback(async () => {
    await updateSettings({ kind: 'enableWhisperGpu' })
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
    void loadSettingsSnapshot().then(applySettingsSnapshot).catch(reportSettingsError)
  }, [applySettingsSnapshot, loadSettingsSnapshot, reportSettingsError])

  const installGpuPrerequisite = useCallback(() => {
    if (gpuPrerequisite == null) return
    void repairManaged(gpuPrerequisite.id)
      .then(loadSettingsSnapshot)
      .then(applySettingsSnapshot)
      .catch(reportSettingsError)
  }, [applySettingsSnapshot, gpuPrerequisite, loadSettingsSnapshot, reportSettingsError])

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

  const parakeetRuns = nextRun?.kind === 'ready' && nextRun.engine.kind === 'parakeet'

  const updateLanguage = useCallback((value: string | null) =>
    updateSettings({ kind: 'language', value }), [updateSettings])
  const updateWhisperModel = useCallback((value: string | null) =>
    updateSettings({ kind: 'whisperModel', value }), [updateSettings])
  const updateRecordSeconds = useCallback((value: number | null) =>
    updateSettings({ kind: 'recordSeconds', value }), [updateSettings])
  const updateWhisperAcceleration = useCallback((value: string | null) =>
    updateSettings({ kind: 'whisperAcceleration', value }), [updateSettings])
  const updateWhisperGpuDevice = useCallback((value: string | null) =>
    updateSettings({ kind: 'whisperGpuDevice', value }), [updateSettings])
  const updateHud = useCallback((value: boolean | null) =>
    updateSettings({ kind: 'hud', value }), [updateSettings])

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
    nextRun,
    whisper,
    lastUsed,
    parakeetRuns,
    selectEngine,
    enableWhisperGpu,
    updateLanguage,
    updateWhisperModel,
    updateRecordSeconds,
    updateWhisperAcceleration,
    updateWhisperGpuDevice,
    updateHud,
    repairLegacy,
    retryShortcutStatus,
    refreshMicrophones,
    refreshReadiness,
    installGpuPrerequisite,
    refreshGpuDevices,
    selectMicrophone,
    testMicrophone,
    reportSettingsError,
  }
}
