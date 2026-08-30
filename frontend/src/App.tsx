import {
  BookOpenText,
  CircleAlert,
  History,
  Home,
  Settings,
} from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { BrandMark, StatusPill } from './app/chrome'
import { messageFrom } from './app/formatting'
import { useElapsedSeconds } from './app/useElapsedSeconds'
import { useSerialPoll } from './hooks/useSerialPoll'
import { DictionaryView } from './dictionary/DictionaryView'
import { HistoryView } from './history/HistoryView'
import { HomeView } from './home/HomeView'
import { SettingsView } from './settings/SettingsView'
import { presentShortcut } from './shortcut'
import {
  addDictionaryEntry,
  getAppStatus,
  getDictionary,
  getHistory,
  removeDictionaryEntry,
  toggleRecording,
} from './tauri'
import type { AppStatus, DictionaryItem, HistoryItem } from './generated/ipc'
import type { ThemeMode, View } from './types'

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
  cleanupName: 'Rules · fillers and punctuation',
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

const navigation: Array<{ id: View; label: string; icon: typeof Home }> = [
  { id: 'home', label: 'Home', icon: Home },
  { id: 'history', label: 'History', icon: History },
  { id: 'dictionary', label: 'Dictionary', icon: BookOpenText },
  { id: 'settings', label: 'Settings', icon: Settings },
]

function App() {
  const [view, setView] = useState<View>('home')
  const [status, setStatus] = useState<AppStatus>(initialStatus)
  const [history, setHistory] = useState<HistoryItem[]>([])
  const [dictionary, setDictionary] = useState<DictionaryItem[]>([])
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem('echo-theme')
    return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
  })
  const [error, setError] = useState<string | null>(null)
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null)
  const previousPhase = useRef('Idle')
  const recordingSeconds = useElapsedSeconds(recordingStartedAt)
  const shortcut = presentShortcut(status.shortcut)

  const loadCollections = useCallback(
    () => Promise.all([getHistory(), getDictionary()]),
    [],
  )
  const applyCollections = useCallback(([nextHistory, nextDictionary]: Awaited<ReturnType<typeof loadCollections>>) => {
    setHistory(nextHistory)
    setDictionary(nextDictionary)
  }, [])
  const refreshCollections = useCallback(async () => {
    applyCollections(await loadCollections())
  }, [applyCollections, loadCollections])

  const applyStatus = useCallback((next: AppStatus) => {
    setStatus(next)
    const observedAt = Date.now()
    setRecordingStartedAt((prev) => (next.recording ? (prev ?? observedAt) : null))
    if (previousPhase.current !== 'Idle' && next.phase === 'Idle') {
      void refreshCollections()
    }
    previousPhase.current = next.phase
  }, [refreshCollections])

  const reportError = useCallback((reason: unknown) => setError(messageFrom(reason)), [])
  const pollWhileVisible = useCallback(() => !document.hidden, [])
  const refreshStatus = useSerialPoll({
    request: getAppStatus,
    onResult: applyStatus,
    onError: reportError,
    intervalMs: 400,
    shouldPoll: pollWhileVisible,
  })

  useEffect(() => {
    let active = true
    void loadCollections().then((collections) => {
      if (active) applyCollections(collections)
    }).catch((reason: unknown) => {
      if (active) reportError(reason)
    })
    return () => {
      active = false
    }
  }, [applyCollections, loadCollections, reportError])

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

  const onToggleRecording = async () => {
    try {
      await toggleRecording()
      await refreshStatus()
    } catch (reason) {
      setError(messageFrom(reason))
    }
  }

  const onAddDictionary = async (spoken: string, written: string) => {
    try {
      await addDictionaryEntry(spoken, written)
      setDictionary(await getDictionary())
    } catch (reason) {
      setError(messageFrom(reason))
      throw reason
    }
  }

  const onRemoveDictionary = async (entry: DictionaryItem) => {
    try {
      const removed = await removeDictionaryEntry(entry.spoken, entry.written)
      if (!removed) setError(`"${entry.spoken}" was already removed.`)
      setDictionary(await getDictionary())
    } catch (reason) {
      setError(messageFrom(reason))
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <BrandMark />
          <h1>Echo</h1>
        </div>
        <div className="topbar-actions">
          <StatusPill status={status} />
        </div>
      </header>

      <div className="workspace">
        <nav className="sidebar" aria-label="Echo sections">
          <div className="nav-list">
            {navigation.map((item) => {
              const Icon = item.icon
              return (
                <button
                  type="button"
                  key={item.id}
                  className="nav-item"
                  data-active={view === item.id}
                  aria-current={view === item.id ? 'page' : undefined}
                  aria-label={item.label}
                  onClick={() => setView(item.id)}
                >
                  <Icon size={18} aria-hidden="true" />
                  <span>{item.label}</span>
                </button>
              )
            })}
          </div>
          <div className="shortcut-card">
            <span>Toggle shortcut</span>
            <kbd>{shortcut.display}</kbd>
            <small>
              {shortcut.ready
                ? 'Press once to start, again to stop.'
                : shortcut.manualCommand
                  ? 'Bind it in your desktop settings.'
                  : 'Open Settings to finish shortcut setup.'}
            </small>
          </div>
        </nav>

        <main className="main-content">
          {error ? (
            <div className="error-banner" role="alert">
              <CircleAlert size={17} aria-hidden="true" />
              <span>{error}</span>
              <button type="button" onClick={() => setError(null)} aria-label="Dismiss error">×</button>
            </div>
          ) : null}

          {view === 'home' ? (
            <HomeView
              status={status}
              history={history}
              recordingSeconds={recordingSeconds}
              onToggleRecording={onToggleRecording}
              onOpenSettings={() => setView('settings')}
            />
          ) : null}
          {view === 'history' ? <HistoryView items={history} /> : null}
          {view === 'dictionary' ? (
            <DictionaryView
              items={dictionary}
              onAdd={onAddDictionary}
              onRemove={onRemoveDictionary}
            />
          ) : null}
          {view === 'settings' ? (
            <SettingsView
              status={status}
              theme={theme}
              onThemeChange={setTheme}
              onStatusChange={refreshStatus}
              onError={setError}
            />
          ) : null}
        </main>
      </div>
    </div>
  )
}

export default App
