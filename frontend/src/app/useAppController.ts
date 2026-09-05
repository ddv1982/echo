import { useCallback, useEffect, useRef, useState } from 'react'

import { useDictionary } from '../dictionary/useDictionary'
import type { AppStatus, RecordingSnapshot } from '../generated/ipc'
import { useHistory } from '../history/useHistory'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { getAppStatus, quitApp, startCapture, stopCapture } from '../tauri'
import type { ThemeMode, View } from '../types'
import { messageFrom } from './formatting'
import {
  acceptRecordingObservation,
  advanceRecordingObservationEpoch,
  createRecordingObservationState,
  type RecordingObservation,
} from './recordingObservation'
import { useElapsedSeconds } from './useElapsedSeconds'

const initialStatus: AppStatus = {
  phase: 'Idle',
  lastTranscript: null,
  lastHistoryId: null,
  microphoneReady: false,
  engineName: 'Checking speech engine…',
  engineReady: false,
  injectionName: 'Checking insertion…',
  injectionReady: false,
  shortcut: { kind: 'probing', desired: 'Super+Alt+Space' },
  hudEnabled: true,
  recordingLimitSeconds: 0,
  recordingPolicy: {
    minimumSeconds: 0,
    defaultSeconds: 0,
    maximumSeconds: 0,
    presetsSeconds: [],
  },
  settingsPath: '',
  version: '',
  lastError: null,
  lastRun: null,
  languageWarning: null,
  recordingInProcess: false,
  recordingSessionId: null,
  captureStopRequested: false,
  recordingRevision: 0,
  currentExe: '',
  firstPathHit: null,
  staleInstalls: [],
}

export function useAppController() {
  const [view, setView] = useState<View>('home')
  const [status, setStatus] = useState<AppStatus>(initialStatus)
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem('echo-theme')
    return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
  })
  const [error, setError] = useState<string | null>(null)
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null)
  const recordingObservation = useRef(createRecordingObservationState(initialStatus))
  const previousHistoryId = useRef<string | null>(null)
  const toggleInFlight = useRef(false)
  const [recordingRequestPending, setRecordingRequestPending] = useState(false)
  const recordingSeconds = useElapsedSeconds(recordingStartedAt)
  const reportError = useCallback((reason: unknown) => setError(messageFrom(reason)), [])
  const {
    items: history,
    remove: deleteHistoryItem,
    clear: clearHistory,
    refresh: refreshHistory,
  } = useHistory(reportError)
  const {
    items: dictionary,
    add: addDictionaryEntry,
    addBatch: addDictionaryEntriesBatch,
    remove: removeDictionaryEntry,
    refresh: refreshDictionary,
  } = useDictionary(reportError)

  const setObservedStatus = useCallback((observation: RecordingObservation) => {
    const previous = recordingObservation.current.snapshot
    const accepted = acceptRecordingObservation(recordingObservation.current, observation)
    if (accepted === recordingObservation.current) return
    recordingObservation.current = accepted
    const next = accepted.snapshot
    setStatus(next)
    const observedAt = Date.now()
    setRecordingStartedAt((current) =>
      next.phase === 'Recording'
        ? (next.recordingSessionId === previous.recordingSessionId ? current ?? observedAt : observedAt)
        : null)
    if (next.lastHistoryId !== null && next.lastHistoryId !== previousHistoryId.current) {
      previousHistoryId.current = next.lastHistoryId
      void refreshHistory().catch(reportError)
    }
    if (previous.phase !== 'Idle' && ['Idle', 'Failed'].includes(next.phase)) {
      void refreshDictionary().catch(reportError)
    }
  }, [refreshDictionary, refreshHistory, reportError])

  const readStatus = useCallback(async (): Promise<Extract<RecordingObservation, { kind: 'poll' }>> => {
    const epoch = recordingObservation.current.epoch
    return { kind: 'poll', epoch, snapshot: await getAppStatus() }
  }, [])
  const applyStatusObservation = useCallback((result: Awaited<ReturnType<typeof readStatus>>) => {
    setObservedStatus(result)
  }, [setObservedStatus])

  const pollWhileVisible = useCallback(() => !document.hidden, [])
  const refreshStatus = useSerialPoll({
    request: readStatus,
    onResult: applyStatusObservation,
    onError: reportError,
    intervalMs: 400,
    shouldPoll: pollWhileVisible,
  })

  useEffect(() => {
    localStorage.setItem('echo-theme', theme)
    const media = window.matchMedia('(prefers-color-scheme: light)')
    const apply = () => {
      const resolved = theme === 'system' ? (media.matches ? 'light' : 'dark') : theme
      document.documentElement.dataset.theme = resolved
      document.documentElement.style.colorScheme = resolved
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [theme])

  useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' })
  }, [view])

  const toggle = useCallback(async () => {
    const { phase, recordingSessionId } = recordingObservation.current.snapshot
    const processing = phase === 'Transcribing' || phase === 'Injecting'
    if (toggleInFlight.current || recordingRequestPending || processing) return
    const stopping = phase === 'Recording'
    toggleInFlight.current = true
    recordingObservation.current = advanceRecordingObservationEpoch(recordingObservation.current)
    setRecordingRequestPending(true)
    try {
      let snapshot: RecordingSnapshot
      if (stopping) {
        if (!recordingSessionId) throw new Error('Recording session is no longer available.')
        snapshot = await stopCapture(recordingSessionId)
      } else {
        snapshot = await startCapture()
      }
      recordingObservation.current = advanceRecordingObservationEpoch(recordingObservation.current)
      setObservedStatus({ kind: 'acknowledgement', snapshot, requestedFrom: recordingSessionId })
      await refreshStatus()
    } catch (reason) {
      reportError(reason)
    } finally {
      toggleInFlight.current = false
      setRecordingRequestPending(false)
    }
  }, [recordingRequestPending, refreshStatus, reportError, setObservedStatus])

  const quit = useCallback(async () => {
    try {
      await quitApp()
    } catch (reason) {
      reportError(reason)
    }
  }, [reportError])

  return {
    view,
    setView,
    status,
    history,
    deleteHistoryItem,
    clearHistory,
    dictionary,
    theme,
    setTheme,
    error,
    setError,
    recordingSeconds,
    recordingRequestPending,
    refreshStatus,
    toggleRecording: toggle,
    quitApp: quit,
    addDictionaryEntry,
    addDictionaryEntriesBatch,
    removeDictionaryEntry,
  }
}
