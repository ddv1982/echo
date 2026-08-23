import {
  BookOpenText,
  Check,
  CircleAlert,
  Clock3,
  Copy,
  History,
  Home,
  Mic,
  Plus,
  Search,
  Settings,
  Trash2,
  Waves,
} from 'lucide-react'
import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  addDictionaryEntry,
  cancelSetup,
  copyText,
  getAppStatus,
  getDictionary,
  getHistory,
  getReadiness,
  getRecordingLevel,
  getShortcutStatus,
  getSettings,
  getMicrophones,
  listLanguages,
  listModels,
  onSetupEvent,
  removeDictionaryEntry,
  removeStaleInstalls,
  repairLegacyShortcut,
  repairManaged,
  retryShortcut,
  setMicrophone,
  setSettings,
  startSetup,
  testInputDevice,
  testMicrophoneFallback,
  toggleRecording,
  verifyManaged,
  removeManaged,
} from './tauri'
import { deriveStats, groupByDay } from './stats'
import { presentShortcut } from './shortcut'
import type {
  AppStatus,
  DictionaryItem,
  HistoryItem,
  LanguageOptions,
  ModelInventory,
  Readiness,
  SetupEvent,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  SettingSource,
  Settings as AppSettings,
  ThemeMode,
  View,
  WhisperModelInfo,
} from './types'

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
  maxRecordSeconds: 60,
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

  const refreshCollections = useCallback(async () => {
    const [nextHistory, nextDictionary] = await Promise.all([getHistory(), getDictionary()])
    setHistory(nextHistory)
    setDictionary(nextDictionary)
  }, [])

  const refreshStatus = useCallback(async () => {
    const next = await getAppStatus()
    setStatus(next)
    const observedAt = Date.now()
    setRecordingStartedAt((prev) => (next.recording ? (prev ?? observedAt) : null))
    if (previousPhase.current !== 'Idle' && next.phase === 'Idle') {
      void refreshCollections()
    }
    previousPhase.current = next.phase
  }, [refreshCollections])

  useEffect(() => {
    // The timeout keeps the initial fetch's setError out of the effect body,
    // which react-hooks/set-state-in-effect would reject.
    const initialTimer = window.setTimeout(() => {
      void Promise.all([refreshStatus(), refreshCollections()]).catch((reason: unknown) => {
        setError(messageFrom(reason))
      })
    }, 0)
    const timer = window.setInterval(() => {
      if (document.hidden) return
      void refreshStatus().catch((reason: unknown) => setError(messageFrom(reason)))
    }, 400)
    return () => {
      window.clearTimeout(initialTimer)
      window.clearInterval(timer)
    }
  }, [refreshCollections, refreshStatus])

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

/// The phase-1 mark, inline so the window, the launcher, and the tray agree.
/// The gradient stops read CSS variables so the bars keep contrast in both
/// themes.
function BrandMark() {
  return (
    <svg viewBox="0 0 1024 1024" className="brand-mark" aria-hidden="true">
      <defs>
        <linearGradient id="brand-gradient" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" className="brand-stop-from" />
          <stop offset="1" className="brand-stop-to" />
        </linearGradient>
      </defs>
      <g fill="url(#brand-gradient)">
        <rect x="176" y="380" width="96" height="200" rx="48" />
        <rect x="320" y="290" width="96" height="380" rx="48" />
        <rect x="464" y="180" width="96" height="600" rx="48" />
        <rect x="608" y="290" width="96" height="380" rx="48" />
        <rect x="752" y="380" width="96" height="200" rx="48" />
        <circle cx="512" cy="856" r="48" />
      </g>
    </svg>
  )
}

/// The mark reduced to its bars, for empty states, in the theme's tertiary
/// text color.
function BarsMotif() {
  return (
    <svg viewBox="0 0 1024 1024" className="bars-motif" aria-hidden="true">
      <g fill="currentColor">
        <rect x="176" y="380" width="96" height="200" rx="48" />
        <rect x="320" y="290" width="96" height="380" rx="48" />
        <rect x="464" y="180" width="96" height="600" rx="48" />
        <rect x="608" y="290" width="96" height="380" rx="48" />
        <rect x="752" y="380" width="96" height="200" rx="48" />
        <circle cx="512" cy="856" r="48" />
      </g>
    </svg>
  )
}

function StatusPill({ status }: { status: AppStatus }) {
  const tone = status.recording
    ? 'recording'
    : status.phase.startsWith('Failed')
      ? 'error'
      : status.phase === 'Idle'
        ? 'ready'
        : 'busy'
  return (
    <div className="status-pill" data-tone={tone} aria-label={`Echo status: ${status.phase}`}>
      <span className="status-dot" aria-hidden="true" />
      {status.phase}
    </div>
  )
}

function HomeView({
  status,
  history,
  recordingSeconds,
  onToggleRecording,
  onOpenSettings,
}: {
  status: AppStatus
  history: HistoryItem[]
  recordingSeconds: number
  onToggleRecording: () => Promise<void>
  onOpenSettings: () => void
}) {
  const shortcut = presentShortcut(status.shortcut)
  const heroState = status.recording
    ? 'recording'
    : status.phase === 'Transcribing'
      ? 'transcribing'
      : 'idle'
  const stateCopy = status.recording
    ? ['Listening…', 'Speak naturally, then press the shortcut again.']
    : status.phase === 'Transcribing'
      ? ['Transcribing locally…', `${status.engineName} is turning your recording into text.`]
      : ['Ready when you are', 'Your audio stays on this machine.']
  return (
    <div className="view-stack">
      <section className="record-hero" data-state={heroState}>
        <div className="hero-glow" aria-hidden="true" />
        <div className="hero-main">
          <button
            className="record-orb"
            type="button"
            onClick={() => void onToggleRecording()}
            aria-label={status.recording ? 'Stop and transcribe' : 'Start recording'}
          >
            <span className="record-ring" aria-hidden="true" />
            {status.recording ? <Waves size={26} /> : <Mic size={26} />}
          </button>
          <div className="hero-copy">
            <div className="readout">
              <span>{status.recording ? 'Listening' : status.phase === 'Transcribing' ? 'Transcribing' : 'Ready'}</span>
              {status.recording ? (
                <span className="readout-timer">
                  {formatDuration(recordingSeconds)} / {formatDuration(status.maxRecordSeconds)}
                </span>
              ) : null}
            </div>
            <h2>{stateCopy[0]}</h2>
            <p>{stateCopy[1]}</p>
            {status.recording ? <LevelBars live={status.recordingInProcess} /> : null}
            <div className="record-actions">
              <div className="shortcut-hint">
                <kbd>{shortcut.display}</kbd>
                <span>
                  {shortcut.ready
                    ? 'works from any app'
                    : 'setup required in Settings'}
                </span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <StaleInstallWarning status={status} />

      <SetupChecklist status={status} onOpenSettings={onOpenSettings} />

      {status.languageWarning ? (
        <div className="attention-strip" role="status">
          <CircleAlert size={16} aria-hidden="true" />
          <span>{status.languageWarning}</span>
          <button type="button" onClick={onOpenSettings}>Open Settings</button>
        </div>
      ) : null}

      <StatsStrip history={history} />

      <div className="home-grid">
        <section className="panel last-transcript">
          <SectionHeading title="Last transcript" subtitle="Most recently inserted text" />
          {status.lastTranscript ? (
            <blockquote>{status.lastTranscript}</blockquote>
          ) : (
            <div className="empty-state compact">
              <BarsMotif />
              <span>Your next transcript will appear here.</span>
            </div>
          )}
        </section>
        <section className="panel recent-panel">
          <SectionHeading title="Recent" subtitle={`${history.length} saved transcript${history.length === 1 ? '' : 's'}`} />
          <div className="recent-list">
            {history.slice(0, 3).map((item) => (
              <div className="recent-row" key={item.id}>
                <p>{item.text}</p>
                <span>{formatTime(item.startedAt)}</span>
              </div>
            ))}
            {history.length === 0 ? (
              <div className="empty-state compact">
                <BarsMotif />
                <span>No history yet.</span>
              </div>
            ) : null}
          </div>
        </section>
      </div>
    </div>
  )
}

const LEVEL_BAR_COUNT = 14

function LevelBars({ live }: { live: boolean }) {
  const [levels, setLevels] = useState<number[]>(() => Array(LEVEL_BAR_COUNT).fill(0))
  useEffect(() => {
    if (!live) return
    const timer = window.setInterval(() => {
      void getRecordingLevel().then((level) => {
        setLevels((previous) => [...previous.slice(1), level])
      })
    }, 60)
    return () => window.clearInterval(timer)
  }, [live])
  return (
    <div className="level-bars" data-live={live} aria-hidden="true">
      {levels.map((level, index) => (
        <span
          key={index}
          className="level-bar"
          style={live ? { height: `${15 + Math.sqrt(Math.min(1, level * 3)) * 85}%` } : undefined}
        />
      ))}
    </div>
  )
}

/// Warn when the running binary is not what a bare `echo-desktop` launch
/// runs: a stale copy (commonly ~/.local/bin from a source install) shadows
/// the packaged one, and upgrades never reach the user. The button removes
/// them in place; the backend re-scans and never takes paths from the
/// webview.
function StaleInstallWarning({ status }: { status: AppStatus }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const shadowed =
    status.staleInstalls.length > 0 ||
    (status.firstPathHit != null && status.firstPathHit !== status.currentExe)
  const paths =
    status.staleInstalls.length > 0
      ? status.staleInstalls
      : shadowed && status.firstPathHit
        ? [status.firstPathHit]
        : []
  if (paths.length === 0) return null
  const remove = async () => {
    setBusy(true)
    setError(null)
    try {
      await removeStaleInstalls()
    } catch (reason) {
      setError(messageFrom(reason))
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="attention-strip stale-install-card" role="alert">
      <CircleAlert size={16} aria-hidden="true" />
      <span>
        {paths.length === 1
          ? 'Another echo-desktop shadows this one: '
          : 'Other echo-desktop copies shadow this one: '}
        {paths.map((path, index) => (
          <span key={path}>
            {index > 0 ? ', ' : ''}
            <code>{path}</code>
          </span>
        ))}
        .{' '}
        <small>
          Or from a terminal: <code>rm -f {paths.join(' ')}</code>, then relaunch.
        </small>
        {error ? <span className="stale-install-error">{error}</span> : null}
      </span>
      <button type="button" className="compact-button" disabled={busy} onClick={() => void remove()}>
        {busy ? 'Removing…' : 'Remove old copies'}
      </button>
    </div>
  )
}

function ShortcutRow({
  status,
  repairing,
  onRepair,
  onRetry,
}: {
  status: AppStatus
  repairing: boolean
  onRepair: () => void
  onRetry: () => Promise<void>
}) {
  const shortcut = presentShortcut(status.shortcut)
  const currentIdentity = shortcut.verificationIdentity
  const [verification, setVerification] = useState<{ at: number; identity: string } | null>(() => {
    const raw = localStorage.getItem('echo-shortcut-verified-at')
    const identity = localStorage.getItem('echo-shortcut-verified-identity')
    return raw && identity ? { at: Number(raw), identity } : null
  })
  const [phase, setPhase] = useState<'idle' | 'arming' | 'listening' | 'timed-out'>('idle')
  const [retrying, setRetrying] = useState(false)
  const attempt = useRef(0)
  const pollTimer = useRef<number | null>(null)
  const timeoutTimer = useRef<number | null>(null)
  const completeVerification = (identity: string) => {
    attempt.current += 1
    if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
    if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    const now = Math.floor(Date.now() / 1000)
    localStorage.setItem('echo-shortcut-verified-at', String(now))
    localStorage.setItem('echo-shortcut-verified-identity', identity)
    setVerification({ at: now, identity })
    setPhase('idle')
  }

  useEffect(() => {
    return () => {
      attempt.current += 1
      if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
      if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    }
  }, [])

  const start = async () => {
    const attemptId = attempt.current + 1
    attempt.current = attemptId
    if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
    if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    setPhase('arming')

    try {
      const baseline = presentShortcut(await getShortcutStatus())
      if (attempt.current !== attemptId) return
      const expectedActivationSource = baseline.expectedActivationSource
      const baselineIdentity = baseline.verificationIdentity
      if (expectedActivationSource == null || baselineIdentity == null) {
        setPhase('timed-out')
        return
      }
      setPhase('listening')

      const poll = async () => {
        const next = presentShortcut(await getShortcutStatus())
        if (attempt.current !== attemptId) return
        if (
          next.activation !== baseline.activation &&
          next.activation?.startsWith(`${expectedActivationSource}:`) === true &&
          next.verificationIdentity === baselineIdentity
        ) {
          completeVerification(baselineIdentity)
          return
        }
        pollTimer.current = window.setTimeout(() => void poll(), 100)
      }
      pollTimer.current = window.setTimeout(() => void poll(), 100)
      timeoutTimer.current = window.setTimeout(() => {
        if (attempt.current !== attemptId) return
        attempt.current += 1
        if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
        setPhase('timed-out')
      }, 10_000)
    } catch {
      if (attempt.current === attemptId) setPhase('timed-out')
    }
  }

  const repair = () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    setVerification(null)
    onRepair()
  }
  const retry = async () => {
    setRetrying(true)
    try {
      await onRetry()
    } finally {
      setRetrying(false)
    }
  }
  const setup = shortcut.gnomeSetup

  return (
    <div className="setting-row">
      <div>
        <strong>Toggle shortcut</strong>
        <span>{shortcut.description}</span>
        {shortcut.manualCommand ? (
          <span>
            Bind <kbd>{shortcut.desired}</kbd> to <code>{shortcut.manualCommand}</code> in your compositor settings.
          </span>
        ) : null}
      </div>
      <div className="setting-actions">
        <kbd>{shortcut.display}</kbd>
        {setup?.state === 'missing' || setup?.state === 'stale' ? (
          <button type="button" className="compact-button" disabled={repairing} onClick={repair}>
            {repairing
              ? 'Updating…'
              : setup.state === 'missing'
                ? 'Set up GNOME shortcut'
                : 'Repair GNOME shortcut'}
          </button>
        ) : null}
        {shortcut.canRetry ? (
          <button type="button" className="compact-button" disabled={retrying} onClick={() => void retry()}>
            {retrying ? 'Retrying…' : 'Retry shortcut'}
          </button>
        ) : null}
        <span className="status-note" data-tone={shortcut.tone}>
          <span className="status-dot" data-tone={shortcut.tone} aria-hidden="true" />
          {shortcut.statusLabel}
        </span>
        {phase === 'listening' ? (
          <span className="status-note" data-tone="ok">
            <span className="status-dot" data-tone="ok" aria-hidden="true" />
            Listening… press your shortcut
          </span>
        ) : (
          <button
            type="button"
            className="compact-button"
            disabled={status.recording || !shortcut.testable}
            onClick={() => void start()}
          >
            Test shortcut
          </button>
        )}
        {phase === 'timed-out' ? (
          <span className="status-note" data-tone="attention">
            <span className="status-dot" data-tone="attention" aria-hidden="true" />
            No keypress seen — check the binding
          </span>
        ) : null}
        {phase === 'idle' && shortcut.testable && verification?.identity === currentIdentity ? (
          <span className="status-note" data-tone="ok">
            <span className="status-dot" data-tone="ok" aria-hidden="true" />
            Verified {new Date(verification.at * 1000).toLocaleDateString()}
          </span>
        ) : null}
      </div>
    </div>
  )
}

function StatsStrip({ history }: { history: HistoryItem[] }) {
  const stats = useMemo(() => deriveStats(history, new Date()), [history])
  if (history.length === 0) return null
  return (
    <section className="stats-strip" aria-label="Usage">
      <div className="stat">
        <strong>{stats.words.toLocaleString()}</strong>
        <span>words dictated</span>
      </div>
      <div className="stat">
        <strong>{stats.sessionsThisWeek}</strong>
        <span>sessions this week</span>
      </div>
      <div className="stat">
        <strong>{stats.dayStreak}</strong>
        <span>day streak</span>
      </div>
    </section>
  )
}

function SetupChecklist({
  status,
  onOpenSettings,
}: {
  status: AppStatus
  onOpenSettings: () => void
}) {
  const [readiness, setReadiness] = useState<Readiness | null>(null)
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const [setupError, setSetupError] = useState<string | null>(null)
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void getReadiness().then(setReadiness).catch((reason: unknown) => setSetupError(messageFrom(reason)))
    void getMicrophones().then(setMicrophones).catch((reason: unknown) => setSetupError(messageFrom(reason)))
    void onSetupEvent((event) => {
      if (event.kind === 'progress') {
        setReadiness((current) => current && applySetupProgress(current, event))
        return
      }
      if (event.kind === 'failed') setSetupError(event.error)
      void getReadiness().then(setReadiness).catch((reason: unknown) => setSetupError(messageFrom(reason)))
    }).then((next) => {
      unlisten = next
    }).catch((reason: unknown) => setSetupError(messageFrom(reason)))
    return () => unlisten?.()
  }, [])
  // Verified, not asserted: only a passing shortcut test completes this.
  const identity = presentShortcut(status.shortcut).verificationIdentity
  const verified = identity != null
    && localStorage.getItem('echo-shortcut-verified-at') !== null
    && localStorage.getItem('echo-shortcut-verified-identity') === identity
  const items = [
    { key: 'mic', done: readiness?.microphoneReady ?? status.microphoneReady, label: 'Microphone ready' },
    { key: 'engine', done: readiness?.speechReady ?? status.engineReady, label: 'Speech engine and model installed' },
    { key: 'dictation', done: readiness?.hasSuccessfulDictation ?? false, label: 'First dictation complete' },
    { key: 'shortcut', done: verified, label: verified ? 'Shortcut verified' : 'Shortcut bound' },
  ]
  if (readiness?.firstRunComplete && verified) return null
  return (
    <section className="panel checklist" aria-label="Finish setup">
      <SectionHeading title="Finish setup" subtitle="The first-run job is one successful dictation." />
      {setupError ? <div role="alert" className="error-banner">{setupError}</div> : null}
      {readiness && !readiness.microphoneReady && microphones ? (
        <div className="first-run-step">
          <strong>1 · Choose and test a microphone</strong>
          <MicrophoneChooser
            snapshot={microphones}
            test={micTest}
            testing={testingMic}
            onRefresh={() => {
              void Promise.all([getMicrophones(), getReadiness()])
                .then(([nextMicrophones, nextReadiness]) => {
                  setMicrophones(nextMicrophones)
                  setReadiness(nextReadiness)
                })
                .catch((reason: unknown) => setSetupError(messageFrom(reason)))
            }}
            onSelect={(id) => {
              setMicTest(null)
              void setMicrophone(id)
                .then((nextMicrophones) => {
                  setMicrophones(nextMicrophones)
                  return getReadiness()
                })
                .then(setReadiness)
                .catch((reason: unknown) => setSetupError(messageFrom(reason)))
            }}
            onTest={(id, fallback) => {
              setTestingMic(true)
              const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
              void run
                .then((result) => {
                  setMicTest(result)
                  return getReadiness()
                })
                .then(setReadiness)
                .catch((reason: unknown) => setSetupError(messageFrom(reason)))
                .finally(() => setTestingMic(false))
            }}
          />
        </div>
      ) : null}
      {readiness && !readiness.speechReady ? (
        <div className="first-run-step">
          <strong>2 · Set up local speech</strong>
          <SpeechSetupSection
            readiness={readiness}
            onRefresh={() => void getReadiness().then(setReadiness)}
            onError={setSetupError}
          />
        </div>
      ) : null}
      {items.map((item) => (
        <div className="checklist-item" data-done={item.done} key={item.key}>
          <span className="checklist-check" aria-hidden="true">
            {item.done ? <Check size={13} /> : null}
          </span>
          <span className="checklist-label">{item.label}</span>
          {!item.done && item.key === 'shortcut' ? (
            <button type="button" className="compact-button" onClick={onOpenSettings}>
              Open Settings
            </button>
          ) : null}
        </div>
      ))}
    </section>
  )
}

// Elapsed time is derived from the start timestamp instead of stored in a
// counter, so nothing needs a synchronous reset when recording stops. The
// clamp covers the render between a new start and the first tick, when `now`
// is still from the previous recording.
function useElapsedSeconds(startedAt: number | null) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (startedAt === null) return
    const timer = window.setInterval(() => setNow(Date.now()), 250)
    return () => window.clearInterval(timer)
  }, [startedAt])
  return startedAt === null ? 0 : Math.max(0, Math.floor((now - startedAt) / 1000))
}

function HistoryView({ items }: { items: HistoryItem[] }) {
  const [query, setQuery] = useState('')
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    return needle ? items.filter((item) => item.text.toLocaleLowerCase().includes(needle)) : items
  }, [items, query])
  const groups = useMemo(() => groupByDay(filtered, new Date()), [filtered])
  return (
    <div className="view-stack">
      <ViewHeader title="History" subtitle="Every successful local transcription, newest first." />
      <label className="search-field">
        <Search size={17} aria-hidden="true" />
        <span className="sr-only">Search history</span>
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search transcripts…" />
      </label>
      {groups.map((group) => (
        <section className="panel transcript-list" aria-live="polite" key={group.label}>
          <h3 className="day-header">{group.label}</h3>
          {group.items.map((item) => <TranscriptRow key={item.id} item={item} />)}
        </section>
      ))}
      {filtered.length === 0 ? (
        <section className="panel transcript-list">
          <div className="empty-state">
            <BarsMotif />
            <strong>{items.length === 0 ? 'No transcripts yet' : 'No matching transcripts'}</strong>
            <span>{items.length === 0 ? 'Dictate once and it lands here.' : 'Try a different search.'}</span>
          </div>
        </section>
      ) : null}
    </div>
  )
}

function TranscriptRow({ item }: { item: HistoryItem }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    await copyText(item.text)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1200)
  }
  return (
    <article className="transcript-row">
      <div className="transcript-main">
        <p>{item.text}</p>
        <div className="metadata-row">
          <span><Clock3 size={13} /> {formatDateTime(item.startedAt)}</span>
          <span>{item.engine}</span>
          <span>{item.inferMs} ms</span>
        </div>
      </div>
      <button className="icon-button" type="button" onClick={() => void copy()} aria-label="Copy transcript">
        {copied ? <Check size={17} /> : <Copy size={17} />}
      </button>
    </article>
  )
}

function DictionaryView({
  items,
  onAdd,
  onRemove,
}: {
  items: DictionaryItem[]
  onAdd: (spoken: string, written: string) => Promise<void>
  onRemove: (item: DictionaryItem) => Promise<void>
}) {
  const [spoken, setSpoken] = useState('')
  const [written, setWritten] = useState('')
  const [saving, setSaving] = useState(false)
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!spoken.trim() || !written.trim()) return
    setSaving(true)
    try {
      await onAdd(spoken, written)
      setSpoken('')
      setWritten('')
    } finally {
      setSaving(false)
    }
  }
  return (
    <div className="view-stack">
      <ViewHeader title="Dictionary" subtitle="Teach Echo names, products, and phrases that Whisper often mishears." />
      <form className="panel dictionary-form" onSubmit={(event) => void submit(event)}>
        <label><span>What Whisper hears</span><input value={spoken} onChange={(event) => setSpoken(event.target.value)} placeholder="clawed code" /></label>
        <div className="mapping-arrow" aria-hidden="true">→</div>
        <label><span>What Echo should write</span><input value={written} onChange={(event) => setWritten(event.target.value)} placeholder="Claude Code" /></label>
        <button className="primary-button compact-button" type="submit" disabled={saving || !spoken.trim() || !written.trim()}><Plus size={17} /> Add</button>
      </form>
      <section className="panel dictionary-list">
        <div className="table-header"><span>Spoken phrase</span><span>Written form</span><span /></div>
        {items.map((item) => (
          <div className="dictionary-row" key={`${item.spoken}-${item.createdAt}`}>
            <code>{item.spoken}</code>
            <strong>{item.written}</strong>
            <button className="icon-button danger-button" type="button" onClick={() => void onRemove(item)} aria-label={`Remove ${item.written}`}><Trash2 size={16} /></button>
          </div>
        ))}
        {items.length === 0 ? <div className="empty-state"><BarsMotif /><strong>Your dictionary is empty</strong><span>Add a phrase above to make transcription more personal.</span></div> : null}
      </section>
    </div>
  )
}

const ENGINE_LABELS: Record<string, string> = {
  whisper: 'Whisper',
  parakeet: 'Parakeet',
  fake: 'Fake',
}

const CLEANUP_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'rules', label: 'Rules' },
] as const

const RECORD_SECOND_PRESETS = [3, 5, 10, 15, 30, 60]

function SettingsView({
  status,
  theme,
  onThemeChange,
  onStatusChange,
  onError,
}: {
  status: AppStatus
  theme: ThemeMode
  onThemeChange: (theme: ThemeMode) => void
  onStatusChange: () => Promise<void>
  onError: (message: string) => void
}) {
  const [settings, setLocalSettings] = useState<AppSettings | null>(null)
  const settingsRef = useRef<AppSettings | null>(null)
  const writeChainRef = useRef(Promise.resolve())
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [inventory, setInventory] = useState<ModelInventory | null>(null)
  const [languages, setLanguages] = useState<LanguageOptions | null>(null)
  const [readiness, setReadiness] = useState<Readiness | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const [repairingLegacyShortcut, setRepairingLegacyShortcut] = useState(false)

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void getSettings()
        .then((next) => {
          settingsRef.current = next
          setLocalSettings(next)
        })
        .catch((reason: unknown) => onError(messageFrom(reason)))
      void getMicrophones().then(setMicrophones).catch((reason: unknown) => onError(messageFrom(reason)))
      void listModels().then(setInventory).catch((reason: unknown) => onError(messageFrom(reason)))
      void listLanguages().then(setLanguages).catch((reason: unknown) => onError(messageFrom(reason)))
      void getReadiness().then(setReadiness).catch((reason: unknown) => onError(messageFrom(reason)))
    }, 0)
    return () => window.clearTimeout(timer)
  }, [onError])

  const refreshMicrophones = useCallback(() => {
    void getMicrophones()
      .then(setMicrophones)
      .catch((reason: unknown) => onError(messageFrom(reason)))
  }, [onError])

  useEffect(() => {
    const interval = window.setInterval(refreshMicrophones, 3_000)
    window.addEventListener('focus', refreshMicrophones)
    return () => {
      window.clearInterval(interval)
      window.removeEventListener('focus', refreshMicrophones)
    }
  }, [refreshMicrophones])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    const timer = window.setTimeout(() => {
      void onSetupEvent((event) => {
        if (event.kind === 'progress') {
          setReadiness((current) => current && applySetupProgress(current, event))
          return
        }
        if (event.kind === 'failed') onError(event.error)
        void Promise.all([getReadiness(), listModels(), getSettings(), listLanguages()])
          .then(([nextReadiness, nextInventory, nextSettings, nextLanguages]) => {
            setReadiness(nextReadiness)
            setInventory(nextInventory)
            settingsRef.current = nextSettings
            setLocalSettings(nextSettings)
            setLanguages(nextLanguages)
            void onStatusChange()
          })
          .catch((reason: unknown) => onError(messageFrom(reason)))
      }).then((fn) => {
        unlisten = fn
      })
    }, 0)
    return () => {
      window.clearTimeout(timer)
      unlisten?.()
    }
  }, [onError, onStatusChange])

  useEffect(() => {
    settingsRef.current = settings
  }, [settings])

  const commit = useCallback(async (next: AppSettings) => {
    try {
      const written = await setSettings(next)
      settingsRef.current = written
      setLocalSettings(written)
      await onStatusChange()
    } catch (reason) {
      onError(messageFrom(reason))
    }
  }, [onError, onStatusChange])

  const patch = useCallback(async <K extends keyof AppSettings>(key: K, value: AppSettings[K]['value']) => {
    const queued = writeChainRef.current.then(async () => {
      const current = settingsRef.current
      if (!current) return
      await commit({ ...current, [key]: { ...current[key], value } })
    })
    writeChainRef.current = queued
    await queued
  }, [commit])

  const repairLegacy = async () => {
    setRepairingLegacyShortcut(true)
    try {
      await repairLegacyShortcut()
      await onStatusChange()
    } catch (reason) {
      onError(messageFrom(reason))
    } finally {
      setRepairingLegacyShortcut(false)
    }
  }

  const recordSecondOptions = RECORD_SECOND_PRESETS
    .concat(settings ? [settings.recordSeconds.effective] : [])
    .filter((secs, index, all) => all.indexOf(secs) === index)
    .sort((left, right) => left - right)
    .map((secs) => ({ value: String(secs), label: `${secs} seconds` }))

  const whisperRuns =
    settings != null &&
    (settings.engine.effective === 'whisper' ||
      (settings.engine.effective === 'auto' &&
        (inventory?.engines.some((engine) => engine.id === 'whisper' && engine.available) ??
          false)))

  return (
    <div className="view-stack">
      <ViewHeader title="Settings" subtitle="Change how Echo records and transcribes, on this machine." />
      <section className="panel settings-section" aria-label="General">
        <SectionHeading title="General" subtitle="The few decisions that matter." />
        {microphones ? (
          <MicrophoneChooser
            snapshot={microphones}
            test={micTest}
            testing={testingMic}
            onRefresh={refreshMicrophones}
            onSelect={(id) => {
              setMicTest(null)
              void setMicrophone(id)
                .then(setMicrophones)
                .then(() => onStatusChange())
                .catch((reason: unknown) => onError(messageFrom(reason)))
            }}
            onTest={(id, fallback) => {
              setTestingMic(true)
              const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
              void run.then(setMicTest).finally(() => setTestingMic(false))
            }}
          />
        ) : (
          <SettingLine label="Microphone" value={status.microphoneReady ? 'Default input available' : 'No default input'} tone={status.microphoneReady ? 'ok' : 'attention'} />
        )}
        {settings && languages ? (
          <LanguageRow
            languages={languages}
            settings={settings}
            status={status}
            onChange={(value) => void patch('language', value)}
          />
        ) : null}
        {status.languageWarning ? (
          <div className="setting-row" role="status">
            <span className="status-note" data-tone="attention">
              <span className="status-dot" data-tone="attention" aria-hidden="true" />
              {status.languageWarning}
            </span>
          </div>
        ) : null}
        {settings && whisperRuns && inventory ? (
          <div className="setting-row">
            <div>
              <strong>Model quality</strong>
              <span>{overrideHintPlain(settings.whisperModel.source, 'Auto runs the best installed model.')}</span>
              {selectedModelMeta(inventory.whisper, settings.whisperModel.effective) ? (
                <span className="model-meta">{selectedModelMeta(inventory.whisper, settings.whisperModel.effective)}</span>
              ) : null}
            </div>
            <select
              aria-label="Model quality"
              value={settings.whisperModel.effective}
              disabled={settings.whisperModel.source === 'env'}
              onChange={(event) => void patch('whisperModel', event.target.value || null)}
            >
              <option value="">Auto · best installed</option>
              {modelOptions(inventory.whisper, settings.whisperModel.effective).map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>
        ) : null}
        {readiness ? (
          <SpeechSetupSection
            readiness={readiness}
            onRefresh={() => void getReadiness().then(setReadiness)}
            onError={onError}
          />
        ) : null}
        <ShortcutRow
          status={status}
          repairing={repairingLegacyShortcut}
          onRepair={() => void repairLegacy()}
          onRetry={async () => {
            try {
              await retryShortcut()
              await onStatusChange()
            } catch (reason) {
              onError(messageFrom(reason))
            }
          }}
        />
        <div className="setting-row">
          <div><strong>Theme</strong><span>Applied to the Echo window only.</span></div>
          <div className="segmented-control" role="group" aria-label="Application theme">
            {(['system', 'light', 'dark'] as ThemeMode[]).map((mode) => (
              <button type="button" key={mode} data-active={theme === mode} onClick={() => onThemeChange(mode)}>{capitalize(mode)}</button>
            ))}
          </div>
        </div>
      </section>
      <details className="panel settings-section advanced-section">
        <summary>Advanced</summary>
        {settings ? (
          <>
            <div className="setting-row">
              <div>
                <strong>Speech engine</strong>
                <span>{overrideHint(settings.engine.source, 'ECHO_ENGINE', 'Which local engine transcribes recordings.')}</span>
              </div>
              <div className="segmented-control" role="group" aria-label="Speech engine">
                {[{ value: 'auto', label: 'Auto' }]
                  .concat(
                    (inventory?.engines ?? []).map((engine) => ({
                      value: engine.id,
                      label: ENGINE_LABELS[engine.id] ?? engine.id,
                    })),
                  )
                  .map((option) => (
                    <button
                      type="button"
                      key={option.value}
                      data-active={settings.engine.effective === option.value}
                      disabled={settings.engine.source === 'env'}
                      onClick={() => void patch('engine', option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
              </div>
            </div>
            {inventory?.engines
              .filter((engine) => !engine.available && engine.id !== 'fake')
              .map((engine) => (
                <div className="setting-row" key={engine.id}>
                  <span className="status-note" data-tone="attention">
                    <span className="status-dot" data-tone="attention" aria-hidden="true" />
                    {engine.id === 'parakeet' ? 'Parakeet' : 'Whisper'}: {engine.reason}
                  </span>
                </div>
              ))}
            <SettingToggle
              label="Recording HUD"
              description="Show the recording capsule while you dictate."
              value={settings.hud.effective}
              source={settings.hud.source}
              envName="ECHO_HUD"
              onChange={(value) => void patch('hud', value)}
            />
            <SettingSelect
              label="Recording length"
              description={`Timed recordings from the command line. Toggle recording still caps at ${status.maxRecordSeconds} seconds.`}
              value={String(settings.recordSeconds.effective)}
              options={recordSecondOptions}
              source={settings.recordSeconds.source}
              envName="ECHO_RECORD_SECONDS"
              onChange={(value) => void patch('recordSeconds', Number(value))}
            />
            <div className="setting-row">
              <div>
                <strong>Cleanup</strong>
                <span>{overrideHint(settings.cleanup.source, 'ECHO_CLEANUP', 'Tidy transcripts: drop um and uh, capitalize, punctuate.')}</span>
              </div>
              <div className="segmented-control" role="group" aria-label="Cleanup">
                {CLEANUP_OPTIONS.map((option) => (
                  <button
                    type="button"
                    key={option.value}
                    data-active={settings.cleanup.effective === option.value}
                    disabled={settings.cleanup.source === 'env'}
                    onClick={() => void patch('cleanup', option.value)}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            </div>
          </>
        ) : null}
        <SettingLine label="Text insertion" value={status.injectionName} tone={status.injectionReady ? 'ok' : 'attention'} />
        <SettingLine label="Resolved engine" value={status.engineName} tone={status.engineReady ? 'ok' : 'attention'} />
        {status.lastError ? (
          <div className="setting-line">
            <div><strong>Last failure</strong><span>{status.lastError}</span></div>
            <span className="status-note" data-tone="attention">
              <span className="status-dot" data-tone="attention" aria-hidden="true" />
              Failed
            </span>
          </div>
        ) : null}
        {status.lastRun ? (
          <>
            <SettingLine label="Last run" value={`${status.lastRun.engine} · ${status.lastRun.inferMs} ms`} />
            {status.lastRun.modelPath ? <SettingLine label="Model file" value={status.lastRun.modelPath} /> : null}
            {status.lastRun.binary ? <SettingLine label="Binary" value={status.lastRun.binary} /> : null}
            {status.lastRun.multilingual != null ? (
              <SettingLine label="Multilingual" value={status.lastRun.multilingual ? 'Yes' : 'No'} />
            ) : null}
            {status.lastRun.vad != null ? (
              <SettingLine label="VAD" value={status.lastRun.vad ? 'On' : 'Off'} />
            ) : null}
            <SettingLine label="Version" value={status.version} />
          </>
        ) : null}
        {status.settingsPath ? (
          <p className="settings-path">Saved at <code>{status.settingsPath}</code></p>
        ) : null}
      </details>
    </div>
  )
}

function selectedModelMeta(models: WhisperModelInfo[], current: string) {
  const model = models.find((candidate) => candidate.name === current)
  if (!model) return null
  return [
    model.family,
    model.multilingual ? 'multilingual' : 'English-only',
    model.quantisation ?? 'full precision',
    formatSize(model.sizeBytes),
  ].join(' · ')
}

function modelOptions(models: WhisperModelInfo[], current: string) {
  const options = models.map((model) => ({
    value: model.name,
    label: [
      model.name,
      model.multilingual ? 'multilingual' : 'English-only',
      model.quantisation ?? 'full precision',
      formatSize(model.sizeBytes),
    ].join(' · '),
  }))
  if (current && !models.some((model) => model.name === current)) {
    options.push({ value: current, label: `${current} · not on disk` })
  }
  return options
}

function formatSize(bytes: number) {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))} MiB`
  return `${Math.round(bytes / 1024)} KiB`
}

function managedStateLabel(component: Readiness['components'][number]) {
  if (component.activity) {
    if (component.activity.phase === 'downloading') {
      const percent = component.activity.totalBytes > 0
        ? Math.floor((component.activity.receivedBytes / component.activity.totalBytes) * 100)
        : 0
      return component.activity.resumedFromBytes > 0
        ? `Downloading ${percent}% · resumed at ${formatSize(component.activity.resumedFromBytes)}`
        : `Downloading ${percent}%`
    }
    return `${capitalize(component.activity.phase)}…`
  }
  switch (component.managed.kind) {
    case 'absent':
      if (component.activeOrigin === 'system') return 'Ready · system runtime'
      if (component.activeOrigin === 'external') return 'Ready · manually installed'
      return component.managed.resumableBytes > 0
        ? `Partial · ${formatSize(component.managed.resumableBytes)} ready to resume`
        : 'Not installed by Echo'
    case 'ready':
      return `Ready · managed by Echo · ${component.managed.version}`
    case 'needs-repair':
      return component.activeOrigin === 'external' || component.activeOrigin === 'system'
        ? `Managed copy needs repair · using ${component.activeOrigin} fallback`
        : `Needs repair · ${component.managed.reason}`
    case 'unsupported':
      return component.managed.reason
  }
}

function applySetupProgress(readiness: Readiness, event: Extract<SetupEvent, { kind: 'progress' }>) {
  return {
    ...readiness,
    activeOperation: event.progress.operationId,
    activeCancellable: true,
    components: readiness.components.map((component) => ({
      ...component,
      activity: component.id === event.progress.component ? event.progress : null,
    })),
  }
}

function SpeechSetupSection({
  readiness,
  onRefresh,
  onError,
}: {
  readiness: Readiness
  onRefresh: () => void
  onError: (message: string) => void
}) {
  const run = (action: Promise<unknown>) => {
    void action.then(onRefresh).catch((reason: unknown) => onError(messageFrom(reason)))
  }
  const recommended = readiness.plans.find((plan) => plan.id === 'recommended')
  const parakeet = readiness.plans.find((plan) => plan.id === 'parakeet')
  const recommendedModel = readiness.components.find(
    (component) => component.id === readiness.recommendedModel,
  )
  const activeOperation = readiness.activeOperation
  return (
    <div className="speech-setup" aria-label="Speech setup">
      <div className="setting-row">
        <div>
          <strong>Speech setup</strong>
          <span>
            Downloads stay in Echo's managed cache, use SHA-256, and never change PATH or external files.
          </span>
          <span>
            Recommended chooses {recommendedModel?.label ?? 'the low-memory model'}
            {readiness.totalMemoryBytes == null
              ? ' because system memory could not be detected.'
              : ` for ${formatSize(readiness.totalMemoryBytes)} system memory.`}
          </span>
          <span>Parakeet setup selects Parakeet. Auto also prefers an available Parakeet setup.</span>
        </div>
        <div className="setting-actions">
          {readiness.managedSupported && recommended && !recommended.satisfied ? (
            <button
              type="button"
              className="compact-button"
              disabled={activeOperation != null || !recommended.diskReady}
              onClick={() => run(startSetup('recommended'))}
            >
              Set up recommended · {formatSize(recommended.downloadBytes)}
            </button>
          ) : null}
          {readiness.managedSupported && parakeet && !parakeet.satisfied ? (
            <button
              type="button"
              className="compact-button"
              disabled={activeOperation != null || !parakeet.diskReady}
              onClick={() => run(startSetup('parakeet'))}
            >
              Set up Parakeet
            </button>
          ) : null}
          {activeOperation && readiness.activeCancellable ? (
            <button type="button" className="compact-button" onClick={() => run(cancelSetup(activeOperation))}>
              Cancel setup
            </button>
          ) : null}
        </div>
        {recommended && !recommended.diskReady && recommended.diskReason ? (
          <span className="status-note" data-tone="attention">Recommended: {recommended.diskReason}</span>
        ) : recommended && recommended.availableBytes != null ? (
          <span className="status-note">
            Recommended needs {formatSize(recommended.requiredFreeBytes)} free · {formatSize(recommended.availableBytes)} available
          </span>
        ) : null}
        {parakeet && !parakeet.satisfied && !parakeet.diskReady && parakeet.diskReason ? (
          <span className="status-note" data-tone="attention">Parakeet: {parakeet.diskReason}</span>
        ) : null}
      </div>
      {!readiness.managedSupported && readiness.unsupportedReason ? (
        <div className="setting-row">
          <span className="status-note" data-tone="attention">{readiness.unsupportedReason}</span>
        </div>
      ) : null}
      {readiness.components.map((component) => (
        <div className="setting-row component-row" key={component.id}>
          <div>
            <strong>{component.label}</strong>
            <span>{managedStateLabel(component)}</span>
            {component.external.map((external) => (
              <span className="offer-url" key={`${external.origin}:${external.path}`}>
                {capitalize(external.origin)} · {external.path}
              </span>
            ))}
          </div>
          <div className="setting-actions">
            {component.activity ? (
              <div
                className="download-track"
                role="progressbar"
                aria-label={`${component.label} ${component.activity.phase}`}
                aria-valuemin={0}
                aria-valuemax={component.activity.totalBytes}
                aria-valuenow={component.activity.receivedBytes}
              >
                <div
                  className="download-fill"
                  style={{
                    width: `${component.activity.totalBytes > 0
                      ? Math.min(100, (component.activity.receivedBytes / component.activity.totalBytes) * 100)
                      : 0}%`,
                  }}
                />
              </div>
            ) : null}
            {component.managed.kind === 'needs-repair' ? (
              <>
                <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(repairManaged(component.id))}>
                  Repair
                </button>
                <button
                  type="button"
                  className="compact-button danger-button"
                  disabled={activeOperation != null}
                  onClick={() => {
                    if (window.confirm(`Remove damaged Echo-managed ${component.label}? External files stay untouched.`)) {
                      run(removeManaged(component.id))
                    }
                  }}
                >
                  Remove damaged copy
                </button>
              </>
            ) : null}
            {component.managed.kind === 'absent' && readiness.managedSupported ? (
              <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(repairManaged(component.id))}>
                {component.managed.resumableBytes > 0
                  ? `Resume · ${formatSize(component.managed.resumableBytes)} saved`
                  : component.external.length > 0
                    ? 'Install managed copy'
                    : 'Install'}
              </button>
            ) : null}
            {component.managed.kind === 'ready' ? (
              <>
                <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(verifyManaged(component.id))}>
                  Verify
                </button>
                <button
                  type="button"
                  className="compact-button danger-button"
                  disabled={activeOperation != null}
                  onClick={() => {
                    if (window.confirm(`Remove Echo-managed ${component.label}? External files stay untouched.`)) {
                      run(removeManaged(component.id))
                    }
                  }}
                >
                  Remove · {formatSize(component.managed.bytes)}
                </button>
              </>
            ) : null}
          </div>
        </div>
      ))}
    </div>
  )
}

const COMMON_LANGUAGE_ORDER = ['en', 'de', 'es', 'fr']

function LanguageRow({
  languages,
  settings,
  status,
  onChange,
}: {
  languages: LanguageOptions
  settings: AppSettings
  status: AppStatus
  onChange: (value: string) => void
}) {
  if (languages.mode === 'parakeet') {
    return (
      <SettingLine
        label="Language"
        value={`Automatic across ${languages.options.length} languages · not reported`}
      />
    )
  }
  if (languages.mode === 'english') {
    return <SettingLine label="Language" value="English" />
  }
  const detected = status.lastRun?.language ?? null
  const common = [
    ...COMMON_LANGUAGE_ORDER.flatMap((code) =>
      languages.options.filter((option) => option.code === code),
    ),
    ...(detected && !COMMON_LANGUAGE_ORDER.includes(detected)
      ? languages.options.filter((option) => option.code === detected)
      : []),
  ]
  const all = [...languages.options].sort((a, b) => a.englishName.localeCompare(b.englishName))
  const detectedOption = detected
    ? languages.options.find((option) => option.code === detected)
    : null
  const probability = status.lastRun?.languageProbability ?? null
  const lowConfidence = probability != null && probability < 0.5
  // A confident detection earns the fast path back: one click pins it.
  const confident = probability != null && probability >= 0.8
  return (
    <label className="setting-row">
      <div>
        <strong>Language</strong>
        <span>
          {overrideHint(
            settings.language.source,
            'ECHO_LANGUAGE',
            'Pin a language, or let Whisper detect it.',
          )}
        </span>
      </div>
      <div className="setting-actions">
        <select
          aria-label="Language"
          value={settings.language.effective}
          disabled={settings.language.source === 'env'}
          onChange={(event) => onChange(event.target.value)}
        >
          <option value="auto">Auto · detect language</option>
          <optgroup label="Common">
            {common.map((option) => (
              <option key={option.code} value={option.code}>
                {capitalize(option.englishName)}
              </option>
            ))}
          </optgroup>
          <optgroup label="All languages">
            {all.map((option) => (
              <option key={option.code} value={option.code}>
                {capitalize(option.englishName)}
              </option>
            ))}
          </optgroup>
        </select>
        {settings.language.effective === 'auto' && detected ? (
          <span className="status-note chip" data-tone={lowConfidence ? 'attention' : 'ok'}>
            <span
              className="status-dot"
              data-tone={lowConfidence ? 'attention' : 'ok'}
              aria-hidden="true"
            />
            {[
              detected,
              detectedOption ? capitalize(detectedOption.englishName) : null,
              probability != null ? `p=${probability.toFixed(2)}` : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        ) : null}
        {settings.language.effective === 'auto' && detected && confident ? (
          <button
            type="button"
            className="compact-button"
            onClick={() => onChange(detected)}
          >
            Pin {detectedOption ? capitalize(detectedOption.englishName) : detected} for speed
          </button>
        ) : null}
      </div>
    </label>
  )
}

function selectedMicrophoneId(snapshot: MicrophoneSnapshot): string | null | undefined {
  switch (snapshot.selection.kind) {
    case 'system-default':
      return null
    case 'selected':
    case 'legacy-match':
      return snapshot.selection.device.id
    case 'missing-with-fallback':
    case 'missing-without-fallback':
    case 'ambiguous-legacy-name':
      return undefined
  }
}

function microphoneMetadata(device: MicrophoneSnapshot['devices'][number]) {
  return [device.manufacturer, device.interfaceType, device.deviceType]
    .filter((value) => value != null && value.length > 0)
    .join(' · ')
}

function microphoneTestMessage(test: MicrophoneTestResult) {
  switch (test.kind) {
    case 'completed':
      return test.outcome === 'heard'
        ? `Input heard on ${test.device.label} · level ${test.peakRms.toFixed(3)}`
        : `No input detected on ${test.device.label}`
    case 'failed':
      return test.device == null ? test.message : `${test.device.label}: ${test.message}`
  }
}

function MicrophoneChooser({
  snapshot,
  test,
  testing,
  onSelect,
  onRefresh,
  onTest,
}: {
  snapshot: MicrophoneSnapshot
  test: MicrophoneTestResult | null
  testing: boolean
  onSelect: (id: string | null) => void
  onRefresh: () => void
  onTest: (id: string | null, fallback: boolean) => void
}) {
  const selectedId = selectedMicrophoneId(snapshot)
  const locked = snapshot.source === 'environment'
  const fallback =
    snapshot.selection.kind === 'missing-with-fallback'
      ? snapshot.selection.fallback
      : snapshot.selection.kind === 'ambiguous-legacy-name'
        ? snapshot.selection.fallback
        : null
  const missing =
    snapshot.selection.kind === 'missing-with-fallback' ||
    snapshot.selection.kind === 'missing-without-fallback'
      ? snapshot.selection.requestedLabel
      : null
  return (
    <div className="setting-row microphone-setting">
      <div>
        <strong>Microphone</strong>
        <span>
          {locked
            ? 'ECHO_MICROPHONE controls this setting.'
            : 'Choose the microphone Echo should use. Similar names stay separate.'}
        </span>
      </div>
      {missing ? (
        <div className="microphone-warning" role="alert">
          <strong>{missing} is disconnected.</strong>
          <span>
            {fallback
              ? `Recording will use the system fallback: ${fallback.label}.`
              : 'No system fallback is available.'}
          </span>
        </div>
      ) : null}
      {snapshot.selection.kind === 'ambiguous-legacy-name' ? (
        <div className="microphone-warning" role="alert">
          <strong>More than one microphone is named {snapshot.selection.name}.</strong>
          <span>Choose the intended device once to save its stable ID.</span>
        </div>
      ) : null}
      <div className="microphone-options" role="radiogroup" aria-label="Microphone">
        <label className="microphone-option" data-selected={selectedId === null}>
          <input
            type="radio"
            name="microphone"
            checked={selectedId === null}
            disabled={locked}
            onChange={() => onSelect(null)}
          />
          <span>
            <strong>Follow system default</strong>
            <small>
              {snapshot.devices.find((device) => device.isDefault)?.label ?? 'No default input'}
            </small>
          </span>
        </label>
        {snapshot.devices.map((device) => (
          <label className="microphone-option" data-selected={selectedId === device.id} key={device.id}>
            <input
              type="radio"
              name="microphone"
              checked={selectedId === device.id}
              disabled={locked}
              onChange={() => onSelect(device.id)}
            />
            <span>
              <strong>
                {device.label}
                {device.isDefault ? ' · Current default' : ''}
              </strong>
              <small>{microphoneMetadata(device) || device.id}</small>
            </span>
          </label>
        ))}
      </div>
      <div className="setting-actions microphone-actions">
        <button type="button" className="compact-button" onClick={onRefresh}>
          Refresh
        </button>
        {selectedId !== undefined ? (
          <button
            type="button"
            className="compact-button"
            disabled={testing}
            onClick={() => onTest(selectedId, false)}
          >
            Test selected
          </button>
        ) : null}
        {fallback ? (
          <button
            type="button"
            className="compact-button"
            disabled={testing}
            onClick={() => onTest(null, true)}
          >
            Test system fallback
          </button>
        ) : null}
        {test ? (
          <span
            className="status-note"
            data-tone={test.kind === 'completed' && test.outcome === 'heard' ? 'ok' : 'attention'}
            role="status"
            aria-live="polite"
          >
            <span
              className="status-dot"
              data-tone={test.kind === 'completed' && test.outcome === 'heard' ? 'ok' : 'attention'}
              aria-hidden="true"
            />
            {microphoneTestMessage(test)}
          </span>
        ) : null}
      </div>
      {snapshot.enumerationWarning ? (
        <span className="status-note" data-tone="attention">
          Some microphones could not be listed: {snapshot.enumerationWarning}
        </span>
      ) : null}
    </div>
  )
}


function overrideHint(source: SettingSource, envName: string, fallback: string) {
  return source === 'env' ? envName : fallback
}

/// General-surface rows name no environment variables; Advanced rows do.
function overrideHintPlain(source: SettingSource, fallback: string) {
  return source === 'env' ? 'Set by environment' : fallback
}

function SettingSelect({
  label,
  description,
  value,
  options,
  source,
  envName,
  onChange,
}: {
  label: string
  description: string
  value: string
  options: Array<{ value: string; label: string }>
  source: SettingSource
  envName: string
  onChange: (value: string) => void
}) {
  const locked = source === 'env'
  return (
    <label className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{locked ? envName : description}</span>
      </div>
      <select
        aria-label={label}
        value={value}
        disabled={locked}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
    </label>
  )
}

function SettingToggle({
  label,
  description,
  value,
  source,
  envName,
  onChange,
}: {
  label: string
  description: string
  value: boolean
  source: SettingSource
  envName: string
  onChange: (value: boolean) => void
}) {
  const locked = source === 'env'
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{locked ? envName : description}</span>
      </div>
      <div className="segmented-control" role="group" aria-label={label}>
        <button type="button" data-active={value} disabled={locked} onClick={() => onChange(true)}>On</button>
        <button type="button" data-active={!value} disabled={locked} onClick={() => onChange(false)}>Off</button>
      </div>
    </div>
  )
}

function ViewHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return <header className="view-header"><h2>{title}</h2><p>{subtitle}</p></header>
}

function SectionHeading({ title, subtitle }: { title: string; subtitle: string }) {
  return <div className="section-heading"><h3>{title}</h3><p>{subtitle}</p></div>
}

type SettingTone = 'ok' | 'attention'

function SettingLine({ label, value, tone }: { label: string; value: string; tone?: SettingTone }) {
  return (
    <div className="setting-line">
      <div><strong>{label}</strong><span>{value}</span></div>
      {tone ? (
        <span className="status-note" data-tone={tone}>
          <span className="status-dot" data-tone={tone} aria-hidden="true" />
          {tone === 'ok' ? 'Ready' : 'Needs setup'}
        </span>
      ) : null}
    </div>
  )
}

function formatDuration(seconds: number) {
  const minutes = Math.floor(seconds / 60)
  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(timestamp * 1000)
}

function formatDateTime(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(timestamp * 1000)
}

function capitalize(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1)
}

function messageFrom(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason)
}

export default App
