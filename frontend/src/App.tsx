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
  copyText,
  getAppStatus,
  getDictionary,
  getHistory,
  removeDictionaryEntry,
  toggleRecording,
} from './tauri'
import type { AppStatus, DictionaryItem, HistoryItem, ThemeMode, View } from './types'

const initialStatus: AppStatus = {
  phase: 'Idle',
  lastTranscript: null,
  recording: false,
  microphoneReady: false,
  engineName: 'Checking speech engine…',
  engineReady: false,
  injectionName: 'Checking insertion…',
  injectionReady: false,
  shortcut: 'Super+Alt+Space',
  cleanupName: 'Rules · fillers and punctuation',
  hudEnabled: true,
  maxRecordSeconds: 60,
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
  const previousPhase = useRef('Idle')

  const refreshCollections = useCallback(async () => {
    const [nextHistory, nextDictionary] = await Promise.all([getHistory(), getDictionary()])
    setHistory(nextHistory)
    setDictionary(nextDictionary)
  }, [])

  const refreshStatus = useCallback(async () => {
    const next = await getAppStatus()
    setStatus(next)
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
            <span>Suggested shortcut</span>
            <kbd>{status.shortcut}</kbd>
            <small>Bind it in your desktop settings. Press once to start, again to stop.</small>
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
            <SettingsView status={status} theme={theme} onThemeChange={setTheme} />
          ) : null}
        </main>
      </div>
    </div>
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
  onToggleRecording,
  onOpenSettings,
}: {
  status: AppStatus
  history: HistoryItem[]
  onToggleRecording: () => Promise<void>
  onOpenSettings: () => void
}) {
  const elapsed = useRecordingSeconds(status.recording)
  const readout = status.recording ? 'Listening' : status.phase === 'Transcribing' ? 'Transcribing' : 'Ready'
  const stateCopy = status.recording
    ? ['Listening…', 'Speak naturally, then press the shortcut again.']
    : status.phase === 'Transcribing'
      ? ['Transcribing locally…', `${status.engineName} is turning your recording into text.`]
      : ['Ready when you are', 'Your audio stays on this machine.']
  const attention = [
    !status.microphoneReady ? 'microphone' : null,
    !status.engineReady ? 'speech engine' : null,
    !status.injectionReady ? 'text insertion' : null,
  ].filter((item): item is string => item !== null)
  return (
    <div className="view-stack">
      <section className="record-panel" data-recording={status.recording}>
        <div className="readout">
          <span>{readout}</span>
          {status.recording ? (
            <span className="readout-timer">{elapsed}s / {status.maxRecordSeconds}s</span>
          ) : null}
        </div>
        <h2>{stateCopy[0]}</h2>
        <p>{stateCopy[1]}</p>
        <div className="record-actions">
          <button className="primary-button" type="button" onClick={() => void onToggleRecording()}>
            {status.recording ? <Waves size={18} /> : <Mic size={18} />}
            {status.recording ? 'Stop & transcribe' : 'Start recording'}
          </button>
          <div className="shortcut-hint">
            <kbd>{status.shortcut}</kbd>
            <span>works from any app</span>
          </div>
        </div>
      </section>

      {attention.length > 0 ? (
        <div className="attention-strip" role="status">
          <CircleAlert size={16} aria-hidden="true" />
          <span>Needs setup: {attention.join(', ')}.</span>
          <button type="button" onClick={onOpenSettings}>Open Settings</button>
        </div>
      ) : null}

      <div className="home-grid">
        <section className="panel last-transcript">
          <SectionHeading title="Last transcript" subtitle="Most recently inserted text" />
          {status.lastTranscript ? (
            <blockquote>{status.lastTranscript}</blockquote>
          ) : (
            <div className="empty-state compact"><span>Your next transcript will appear here.</span></div>
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
            {history.length === 0 ? <div className="empty-state compact">No history yet.</div> : null}
          </div>
        </section>
      </div>
    </div>
  )
}

function useRecordingSeconds(recording: boolean) {
  const [seconds, setSeconds] = useState(0)
  useEffect(() => {
    if (!recording) {
      setSeconds(0)
      return
    }
    const startedAt = Date.now()
    const timer = window.setInterval(() => {
      setSeconds(Math.floor((Date.now() - startedAt) / 1000))
    }, 250)
    return () => window.clearInterval(timer)
  }, [recording])
  return seconds
}

function HistoryView({ items }: { items: HistoryItem[] }) {
  const [query, setQuery] = useState('')
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    return needle ? items.filter((item) => item.text.toLocaleLowerCase().includes(needle)) : items
  }, [items, query])
  return (
    <div className="view-stack">
      <ViewHeader title="History" subtitle="Every successful local transcription, newest first." />
      <label className="search-field">
        <Search size={17} aria-hidden="true" />
        <span className="sr-only">Search history</span>
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search transcripts…" />
      </label>
      <section className="panel transcript-list" aria-live="polite">
        {filtered.map((item) => <TranscriptRow key={item.id} item={item} />)}
        {filtered.length === 0 ? (
          <div className="empty-state"><History size={28} /><strong>No matching transcripts</strong><span>Try a different search.</span></div>
        ) : null}
      </section>
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
        {items.length === 0 ? <div className="empty-state"><BookOpenText size={28} /><strong>Your dictionary is empty</strong><span>Add a phrase above to make transcription more personal.</span></div> : null}
      </section>
    </div>
  )
}

function SettingsView({ status, theme, onThemeChange }: { status: AppStatus; theme: ThemeMode; onThemeChange: (theme: ThemeMode) => void }) {
  return (
    <div className="view-stack">
      <ViewHeader title="Settings" subtitle="Review the local components Echo uses for dictation." />
      <section className="panel settings-section">
        <SectionHeading title="Appearance" subtitle="Follow the system or choose a fixed theme." />
        <div className="setting-row">
          <div><strong>Theme</strong><span>Applied to the Echo window only.</span></div>
          <div className="segmented-control" role="group" aria-label="Application theme">
            {(['system', 'light', 'dark'] as ThemeMode[]).map((mode) => (
              <button type="button" key={mode} data-active={theme === mode} onClick={() => onThemeChange(mode)}>{capitalize(mode)}</button>
            ))}
          </div>
        </div>
      </section>
      <section className="panel settings-section">
        <SectionHeading title="Shortcut & recording" subtitle="Bind the suggested shortcut in your desktop's keyboard settings; Echo does not register it itself." />
        <SettingLine label="Suggested shortcut" value={status.shortcut} badge="Toggle" />
        <SettingLine label="Recording HUD" value={status.hudEnabled ? 'Echo pulse capsule (X11 sessions)' : 'Disabled via ECHO_HUD'} badge={status.hudEnabled ? 'On' : 'Off'} />
        <SettingLine label="Maximum recording" value={`${status.maxRecordSeconds} seconds`} />
      </section>
      <section className="panel settings-section">
        <SectionHeading title="Local pipeline" subtitle="No recorded audio leaves this machine." />
        <SettingLine label="Speech engine" value={status.engineName} badge={status.engineReady ? 'Ready' : 'Setup'} />
        <SettingLine label="Microphone" value={status.microphoneReady ? 'Default input available' : 'No default input'} badge={status.microphoneReady ? 'Ready' : 'Check'} />
        <SettingLine label="Text insertion" value={status.injectionName} badge={status.injectionReady ? 'Ready' : 'Check'} />
        <SettingLine label="Cleanup" value={status.cleanupName} />
      </section>
    </div>
  )
}

function ViewHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return <header className="view-header"><h2>{title}</h2><p>{subtitle}</p></header>
}

function SectionHeading({ title, subtitle }: { title: string; subtitle: string }) {
  return <div className="section-heading"><h3>{title}</h3><p>{subtitle}</p></div>
}

function SettingLine({ label, value, badge }: { label: string; value: string; badge?: string }) {
  return <div className="setting-line"><div><strong>{label}</strong><span>{value}</span></div>{badge ? <span className="small-badge">{badge}</span> : null}</div>
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
