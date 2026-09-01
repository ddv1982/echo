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
  recording: false,
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

export function useAppController() {
  const [view, setView] = useState<View>('home')
  const [status, setStatus] = useState<AppStatus>(initialStatus)
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem('echo-theme')
    return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
  })
  const [error, setError] = useState<string | null>(null)
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null)
  const previousPhase = useRef('Idle')
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
    const observedAt = Date.now()
    setRecordingStartedAt((current) => (next.recording ? (current ?? observedAt) : null))
    if (previousPhase.current !== 'Idle' && next.phase === 'Idle') {
      void Promise.all([refreshHistory(), refreshDictionary()])
    }
    previousPhase.current = next.phase
  }, [refreshDictionary, refreshHistory])

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
    try {
      await toggleRecording()
      await refreshStatus()
    } catch (reason) {
      reportError(reason)
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
    refreshStatus,
    toggleRecording: toggle,
    quitApp: quit,
    addDictionaryEntry,
    addDictionaryEntriesBatch,
    removeDictionaryEntry,
  }
}
