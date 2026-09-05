import { useCallback, useEffect, useRef, useState } from 'react'

import { useDictionary } from '../dictionary/useDictionary'
import type { AppStatus } from '../generated/ipc'
import { useHistory } from '../history/useHistory'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { getAppStatus, quitApp, toggleRecording } from '../tauri'
import type { ThemeMode, View } from '../types'
import { messageFrom } from './formatting'
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
  currentExe: '',
  firstPathHit: null,
  staleInstalls: [],
}

type StopState = 'none' | 'requesting' | 'awaiting-status'

export function useAppController() {
  const [view, setView] = useState<View>('home')
  const [status, setStatus] = useState<AppStatus>(initialStatus)
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem('echo-theme')
    return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
  })
  const [error, setError] = useState<string | null>(null)
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null)
  const previousPhase = useRef<AppStatus['phase']>('Idle')
  const previousHistoryId = useRef<string | null>(null)
  const toggleInFlight = useRef(false)
  const stopStateRef = useRef<StopState>('none')
  const [stopState, setStopState] = useState<StopState>('none')
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

  const applyStatus = useCallback((next: AppStatus) => {
    setStatus(next)
    if (stopStateRef.current === 'awaiting-status' && next.phase !== 'Recording') {
      stopStateRef.current = 'none'
      setStopState('none')
    }
    const observedAt = Date.now()
    setRecordingStartedAt((current) =>
      next.phase === 'Recording' ? (current ?? observedAt) : null)
    if (next.lastHistoryId !== null && next.lastHistoryId !== previousHistoryId.current) {
      previousHistoryId.current = next.lastHistoryId
      void refreshHistory().catch(reportError)
    }
    if (previousPhase.current !== 'Idle' && ['Idle', 'Failed'].includes(next.phase)) {
      void refreshDictionary().catch(reportError)
    }
    previousPhase.current = next.phase
  }, [refreshDictionary, refreshHistory, reportError])

  const pollWhileVisible = useCallback(() => !document.hidden, [])
  const refreshStatus = useSerialPoll({
    request: getAppStatus,
    onResult: applyStatus,
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
    const phase = previousPhase.current
    const processing = phase === 'Transcribing' || phase === 'Injecting'
    if (toggleInFlight.current || stopStateRef.current !== 'none' || processing) return
    const stopping = phase === 'Recording'
    if (stopping) {
      stopStateRef.current = 'requesting'
      setStopState('requesting')
    }
    toggleInFlight.current = true
    try {
      await toggleRecording()
      if (stopping) {
        stopStateRef.current = 'awaiting-status'
        setStopState('awaiting-status')
      }
      await refreshStatus()
    } catch (reason) {
      if (stopping) {
        stopStateRef.current = 'none'
        setStopState('none')
      }
      reportError(reason)
    } finally {
      toggleInFlight.current = false
    }
  }, [refreshStatus, reportError])

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
    stopPending: stopState !== 'none',
    refreshStatus,
    toggleRecording: toggle,
    quitApp: quit,
    addDictionaryEntry,
    addDictionaryEntriesBatch,
    removeDictionaryEntry,
  }
}
