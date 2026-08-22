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
  cancelDownload,
  downloadModel,
  getSettings,
  listInputDevices,
  listLanguages,
  listModelOffers,
  listModels,
  onDownloadProgress,
  removeDictionaryEntry,
  setSettings,
  testInputDevice,
  toggleRecording,
} from './tauri'
import type {
  AppStatus,
  DictionaryItem,
  DownloadProgress,
  HistoryItem,
  LanguageOptions,
  ModelInventory,
  ModelOffer,
  SettingSource,
  Settings as AppSettings,
  ThemeMode,
  View,
  InputDevice,
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
  shortcut: 'Super+Alt+Space',
  cleanupName: 'Rules · fillers and punctuation',
  hudEnabled: true,
  maxRecordSeconds: 60,
  settingsPath: '',
  version: '',
  lastError: null,
  lastRun: null,
  languageWarning: null,
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
            <span className="readout-timer">{recordingSeconds}s / {status.maxRecordSeconds}s</span>
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

      {status.languageWarning ? (
        <div className="attention-strip" role="status">
          <CircleAlert size={16} aria-hidden="true" />
          <span>{status.languageWarning}</span>
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
          <div className="empty-state"><strong>No matching transcripts</strong><span>Try a different search.</span></div>
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
        {items.length === 0 ? <div className="empty-state"><strong>Your dictionary is empty</strong><span>Add a phrase above to make transcription more personal.</span></div> : null}
      </section>
    </div>
  )
}

const ENGINE_OPTIONS = [
  { value: 'auto', label: 'Auto' },
  { value: 'whisper', label: 'Whisper' },
  { value: 'parakeet', label: 'Parakeet' },
  { value: 'fake', label: 'Fake' },
] as const

const CLEANUP_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'rules', label: 'Rules' },
] as const

const HOLD_KEY_OPTIONS = [
  { value: 'RightCtrl', label: 'Right Ctrl' },
  { value: 'LeftCtrl', label: 'Left Ctrl' },
  { value: 'RightShift', label: 'Right Shift' },
  { value: 'LeftShift', label: 'Left Shift' },
  { value: 'Super', label: 'Super' },
  { value: 'Alt', label: 'Alt' },
  { value: 'Space', label: 'Space' },
]

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
  const [devices, setDevices] = useState<InputDevice[]>([])
  const [inventory, setInventory] = useState<ModelInventory | null>(null)
  const [languages, setLanguages] = useState<LanguageOptions | null>(null)
  const [offers, setOffers] = useState<ModelOffer[]>([])
  const [downloads, setDownloads] = useState<Record<string, DownloadProgress>>({})
  const [micMeter, setMicMeter] = useState<number | 'unavailable' | null>(null)
  const [testingMic, setTestingMic] = useState(false)

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void Promise.all([
        getSettings(),
        listInputDevices(),
        listModels(),
        listLanguages(),
        listModelOffers(),
      ]).then(([next, listed, models, languageOptions, modelOffers]) => {
        setLocalSettings(next)
        setDevices(listed)
        setInventory(models)
        setLanguages(languageOptions)
        setOffers(modelOffers)
      })
    }, 0)
    return () => window.clearTimeout(timer)
  }, [])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    const timer = window.setTimeout(() => {
      void onDownloadProgress((progress) => {
        setDownloads((previous) => ({ ...previous, [progress.id]: progress }))
        if (progress.stage === 'done') {
          void listModelOffers().then(setOffers)
          void listModels().then(setInventory)
        }
      }).then((fn) => {
        unlisten = fn
      })
    }, 0)
    return () => {
      window.clearTimeout(timer)
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    settingsRef.current = settings
  }, [settings])

  const commit = async (next: AppSettings) => {
    try {
      const written = await setSettings(next)
      settingsRef.current = written
      setLocalSettings(written)
      await onStatusChange()
    } catch (reason) {
      onError(messageFrom(reason))
    }
  }

  const patch = async <K extends keyof AppSettings>(key: K, value: AppSettings[K]['value']) => {
    const queued = writeChainRef.current.then(async () => {
      const current = settingsRef.current
      if (!current) return
      await commit({ ...current, [key]: { ...current[key], value } })
    })
    writeChainRef.current = queued
    await queued
  }

  const recordSecondOptions = RECORD_SECOND_PRESETS
    .concat(settings ? [settings.recordSeconds.effective] : [])
    .filter((secs, index, all) => all.indexOf(secs) === index)
    .sort((left, right) => left - right)
    .map((secs) => ({ value: String(secs), label: `${secs} seconds` }))

  return (
    <div className="view-stack">
      <ViewHeader title="Settings" subtitle="Change how Echo records and transcribes, on this machine." />
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
        <SectionHeading title="Audio" subtitle="Input stays on this machine." />
        {settings ? (
          <div className="setting-row">
            <div>
              <strong>Microphone</strong>
              <span>{microphoneHint(settings, devices)}</span>
            </div>
            <div className="setting-actions">
              <select
                aria-label="Microphone"
                value={settings.microphone.effective}
                disabled={settings.microphone.source === 'env'}
                onChange={(event) => {
                  setMicMeter(null)
                  void patch('microphone', event.target.value || null)
                }}
              >
                <option value="">System default</option>
                {microphoneOptions(devices, settings.microphone.effective).map((device) => (
                  <option key={device.name} value={device.name}>{device.name}</option>
                ))}
              </select>
              <button
                type="button"
                className="compact-button"
                disabled={testingMic}
                onClick={() => {
                  setTestingMic(true)
                  void testInputDevice(settings.microphone.effective || null)
                    .then(setMicMeter)
                    .catch(() => setMicMeter('unavailable'))
                    .finally(() => setTestingMic(false))
                }}
              >
                Test
              </button>
              {micMeter === 'unavailable' ? (
                <span className="status-note" data-tone="attention">
                  <span className="status-dot" data-tone="attention" aria-hidden="true" />
                  Unavailable
                </span>
              ) : micMeter != null ? (
                <span className="status-note" data-tone={micMeter > 0.001 ? 'ok' : 'attention'}>
                  <span className="status-dot" data-tone={micMeter > 0.001 ? 'ok' : 'attention'} aria-hidden="true" />
                  {micMeter > 0.001 ? `Level ${micMeter.toFixed(3)}` : 'Silent'}
                </span>
              ) : (
                <span className="status-note" data-tone={status.microphoneReady ? 'ok' : 'attention'}>
                  <span className="status-dot" data-tone={status.microphoneReady ? 'ok' : 'attention'} aria-hidden="true" />
                  {status.microphoneReady ? 'Ready' : 'Needs setup'}
                </span>
              )}
            </div>
          </div>
        ) : (
          <SettingLine label="Microphone" value={status.microphoneReady ? 'Default input available' : 'No default input'} tone={status.microphoneReady ? 'ok' : 'attention'} />
        )}
      </section>
      <section className="panel settings-section">
        <SectionHeading title="Transcription" subtitle="No recorded audio leaves this machine." />
        {settings ? (
          <>
            <div className="setting-row">
              <div>
                <strong>Speech engine</strong>
                <span>{overrideHint(settings.engine.source, 'ECHO_ENGINE', 'Which local engine transcribes recordings.')}</span>
              </div>
              <div className="segmented-control" role="group" aria-label="Speech engine">
                {ENGINE_OPTIONS.map((option) => (
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
            {settings.engine.effective === 'whisper' && inventory ? (
              <label className="setting-row">
                <div>
                  <strong>Model</strong>
                  <span>{overrideHint(settings.whisperModel.source, 'ECHO_WHISPER_MODEL', 'Auto runs the best installed model.')}</span>
                </div>
                <select
                  aria-label="Model"
                  value={settings.whisperModel.effective}
                  disabled={settings.whisperModel.source === 'env'}
                  onChange={(event) => void patch('whisperModel', event.target.value || null)}
                >
                  <option value="">Auto · best installed</option>
                  {modelOptions(inventory.whisper, settings.whisperModel.effective).map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            ) : null}
            {languages ? (
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
                {offers
                  .filter((offer) => offer.id === 'small' && !offer.installed)
                  .map((offer) => (
                    <OfferAction
                      key={offer.id}
                      offer={offer}
                      progress={downloads[offer.id]}
                      onDownload={() => void downloadModel(offer.id)}
                      onCancel={() => void cancelDownload(offer.id)}
                    />
                  ))}
              </div>
            ) : null}
            {inventory && inventory.vad.length === 0
              ? offers
                  .filter((offer) => offer.id === 'silero-vad' && !offer.installed)
                  .map((offer) => (
                    <div className="setting-row" key={offer.id}>
                      <div>
                        <strong>Silence detection</strong>
                        <span>
                          VAD trims non-speech before transcription. {offer.filename} ·{' '}
                          {formatSize(offer.sizeBytes)}
                        </span>
                      </div>
                      <OfferAction
                        offer={offer}
                        progress={downloads[offer.id]}
                        onDownload={() => void downloadModel(offer.id)}
                        onCancel={() => void cancelDownload(offer.id)}
                      />
                    </div>
                  ))
              : null}
            {offers.filter(
              (offer) =>
                offer.id !== 'silero-vad' &&
                !(status.languageWarning && offer.id === 'small') &&
                (!offer.installed || downloads[offer.id]),
            ).length > 0 ? (
              <>
                <div className="setting-row">
                  <div>
                    <strong>Get a model</strong>
                    <span>Downloaded over HTTPS from huggingface.co and verified against the published SHA-1.</span>
                  </div>
                </div>
                {offers
                  .filter(
                    (offer) =>
                      offer.id !== 'silero-vad' &&
                      !(status.languageWarning && offer.id === 'small') &&
                      (!offer.installed || downloads[offer.id]),
                  )
                  .map((offer) => (
                    <div className="setting-row" key={offer.id}>
                      <div>
                        <strong>{offer.label}</strong>
                        <span>
                          {offer.filename} · {formatSize(offer.sizeBytes)}
                          {offer.runtimeMb != null ? ` · ~${offer.runtimeMb} MB memory` : ''}
                          {offer.multilingual ? ' · multilingual' : ''}
                        </span>
                        <span className="offer-url">{offer.url}</span>
                      </div>
                      <OfferAction
                        offer={offer}
                        progress={downloads[offer.id]}
                        onDownload={() => void downloadModel(offer.id)}
                        onCancel={() => void cancelDownload(offer.id)}
                      />
                    </div>
                  ))}
              </>
            ) : null}
          </>
        ) : null}
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
      </section>
      <section className="panel settings-section">
        <SectionHeading title="Shortcut and recording" subtitle="Bind the suggested shortcut in your desktop's keyboard settings; Echo does not register it itself." />
        <SettingLine label="Suggested shortcut" value={`${status.shortcut} · press once to start, again to stop`} />
        {settings ? (
          <>
            <SettingToggle
              label="Recording HUD"
              description="Echo pulse capsule on X11 sessions."
              value={settings.hud.effective}
              source={settings.hud.source}
              envName="ECHO_HUD"
              onChange={(value) => void patch('hud', value)}
            />
            <SettingSelect
              label="Hold key"
              description="Used by rec --hold. Combos belong on a desktop shortcut."
              value={settings.holdKey.effective}
              options={holdKeyOptions(settings.holdKey.effective)}
              source={settings.holdKey.source}
              envName="ECHO_HOLD_KEY"
              onChange={(value) => void patch('holdKey', value)}
            />
            <SettingSelect
              label="Timed recording"
              description={`Used by rec --once. Toggle recording still caps at ${status.maxRecordSeconds} seconds.`}
              value={String(settings.recordSeconds.effective)}
              options={recordSecondOptions}
              source={settings.recordSeconds.source}
              envName="ECHO_RECORD_SECONDS"
              onChange={(value) => void patch('recordSeconds', Number(value))}
            />
          </>
        ) : null}
      </section>
      <section className="panel settings-section">
        <SectionHeading title="Text" subtitle="What Echo writes after transcription." />
        {settings ? (
          <div className="setting-row">
            <div>
              <strong>Cleanup</strong>
              <span>{overrideHint(settings.cleanup.source, 'ECHO_CLEANUP', status.cleanupName)}</span>
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
        ) : null}
        <SettingLine label="Text insertion" value={status.injectionName} tone={status.injectionReady ? 'ok' : 'attention'} />
      </section>
      {status.settingsPath ? (
        <p className="settings-path">Saved at <code>{status.settingsPath}</code></p>
      ) : null}
    </div>
  )
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

function OfferAction({
  offer,
  progress,
  onDownload,
  onCancel,
}: {
  offer: ModelOffer
  progress: DownloadProgress | undefined
  onDownload: () => void
  onCancel: () => void
}) {
  if (!progress || progress.stage === 'cancelled') {
    return (
      <button type="button" className="compact-button" onClick={onDownload}>
        Download
      </button>
    )
  }
  if (progress.stage === 'failed') {
    return (
      <div className="setting-actions">
        <span className="status-note" data-tone="attention">
          <span className="status-dot" data-tone="attention" aria-hidden="true" />
          {progress.error ?? 'Download failed'}
        </span>
        <button type="button" className="compact-button" onClick={onDownload}>
          Retry
        </button>
      </div>
    )
  }
  if (progress.stage === 'done') {
    return (
      <span className="status-note" data-tone="ok">
        <span className="status-dot" data-tone="ok" aria-hidden="true" />
        Installed
      </span>
    )
  }
  const percent =
    progress.total > 0 ? Math.min(100, Math.floor((progress.received / progress.total) * 100)) : 0
  return (
    <div className="setting-actions">
      <div
        className="download-track"
        role="progressbar"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={`Downloading ${offer.filename}`}
      >
        <div className="download-fill" style={{ width: `${percent}%` }} />
      </div>
      <span className="status-note">
        {progress.stage === 'verifying' ? 'Verifying…' : `${percent}%`}
      </span>
      <button type="button" className="compact-button" onClick={onCancel}>
        Cancel
      </button>
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
          <span className="status-note" data-tone={lowConfidence ? 'attention' : 'ok'}>
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
      </div>
    </label>
  )
}

function microphoneOptions(devices: InputDevice[], current: string) {
  if (!current || devices.some((device) => device.name === current)) return devices
  return [...devices, { name: current, isDefault: false }]
}

function microphoneHint(settings: AppSettings, devices: InputDevice[]) {
  if (settings.microphone.source === 'env') return 'ECHO_MICROPHONE'
  const requested = settings.microphone.effective
  if (!requested) return 'Follow the system default input.'
  if (devices.some((device) => device.name === requested)) {
    return 'Used for GUI recording and rec --toggle.'
  }
  const fallback = devices.find((device) => device.isDefault)?.name ?? 'the system default'
  return `${requested} is gone; using ${fallback}`
}

function holdKeyOptions(current: string) {
  if (HOLD_KEY_OPTIONS.some((option) => option.value === current)) return HOLD_KEY_OPTIONS
  return [...HOLD_KEY_OPTIONS, { value: current, label: current }]
}

function overrideHint(source: SettingSource, envName: string, fallback: string) {
  return source === 'env' ? envName : fallback
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
