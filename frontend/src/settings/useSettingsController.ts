import { useCallback, useEffect, useRef, useState } from 'react'

import { messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { newestSnapshot } from './snapshotFreshness'
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

export function useSettingsController({
  onStatusChange,
  onError,
}: UseSettingsControllerArgs) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null)
  const pendingSettingsWrites = useRef(0)
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

  const loadSettingsSnapshot = useCallback(async (): Promise<SettingsSnapshot | null> => {
    const next = await getSettings()
    return active.current ? next : null
  }, [])

  const applySettingsSnapshot = useCallback((next: SettingsSnapshot | null) => {
    if (next != null && active.current) setSnapshot((current) => newestSnapshot(current, next))
  }, [])

  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
      micTestVersion.current += 1
    }
  }, [])

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

  const applyMicrophoneSnapshot = useCallback((next: MicrophoneSnapshot) => {
    if (active.current) setMicrophones((current) => newestSnapshot(current, next))
  }, [])

  const refreshMicrophones = useSerialPoll({
    request: getMicrophones,
    onResult: applyMicrophoneSnapshot,
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
  const refreshSettingsEvent = useCallback(() =>
    loadSettingsSnapshot().then((result) => async () => {
      applySettingsSnapshot(result)
      await onStatusChange()
    }), [applySettingsSnapshot, loadSettingsSnapshot, onStatusChange])
  const getSettingsEventRefresh = useCallback(() => refreshSettingsEvent, [refreshSettingsEvent])
  const handleSettingsEvent = useCallback(() => undefined, [])
  useAsyncSubscription<void>({
    subscribe: onSettingsEvent,
    onEvent: handleSettingsEvent,
    getRefresh: getSettingsEventRefresh,
    initialRefresh: refreshSettingsEvent,
    onError: reportSettingsError,
  })

  const commit = useCallback(async (change: SettingsChange) => {
    try {
      const written = await setSettings(change)
      if (!active.current) return
      applySettingsSnapshot(written)
      await onStatusChange()
    } catch (reason) {
      reportSettingsError(reason)
      throw reason
    }
  }, [applySettingsSnapshot, onStatusChange, reportSettingsError])

  const updateSettings = useCallback(async (change: SettingsChange) => {
    setSettingsWritePending(true)
    pendingSettingsWrites.current += 1
    try {
      await commit(change)
    } catch {
      const next = await loadSettingsSnapshot().catch((reason: unknown) => {
        reportSettingsError(reason)
        return null
      })
      applySettingsSnapshot(next)
    } finally {
      pendingSettingsWrites.current = Math.max(0, pendingSettingsWrites.current - 1)
      if (active.current && pendingSettingsWrites.current === 0) setSettingsWritePending(false)
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
        applyMicrophoneSnapshot(next)
        return onStatusChange()
      })
      .catch(reportSettingsError)
  }, [applyMicrophoneSnapshot, onStatusChange, reportSettingsError])

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
