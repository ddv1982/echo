import { Check, CircleAlert, Mic, Waves } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { BarsMotif, SectionHeading } from '../app/chrome'
import { formatDuration, formatTime, messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { MicrophoneChooser } from '../settings/MicrophoneChooser'
import { SpeechSetupSection } from '../settings/SpeechSetupSection'
import { applySetupProgress } from '../setup'
import { presentShortcut } from '../shortcut'
import { deriveStats } from '../stats'
import {
  getMicrophones,
  getReadiness,
  getRecordingLevel,
  onSetupEvent,
  removeStaleInstalls,
  setMicrophone,
  testInputDevice,
  testMicrophoneFallback,
} from '../tauri'
import type {
  AppStatus,
  HistoryItem,
  MicrophoneSnapshot,
  MicrophoneTestResult,
  Readiness,
  SetupEvent,
} from '../generated/ipc'

export function HomeView({
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
                  {status.recordingLimitSeconds == null
                    ? formatDuration(recordingSeconds)
                    : `${formatDuration(Math.min(recordingSeconds, status.recordingLimitSeconds))} / ${formatDuration(status.recordingLimitSeconds)}`}
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
  const addLevel = useCallback((level: number) => {
    setLevels((previous) => [...previous.slice(1), level])
  }, [])
  useSerialPoll({
    request: getRecordingLevel,
    onResult: addLevel,
    intervalMs: 60,
    enabled: live,
  })
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

// Warn when the running binary is not what a bare `echo-desktop` launch
// runs: a stale copy (commonly ~/.local/bin from a source install) shadows
// the packaged one, and upgrades never reach the user. The button removes
// them in place; the backend re-scans and never takes paths from the webview.
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
  const micTestVersion = useRef(0)
  const reportSetupError = useCallback((reason: unknown) => {
    setSetupError(messageFrom(reason))
  }, [])
  useEffect(() => {
    let active = true
    void getReadiness().then((next) => {
      if (active) setReadiness(next)
    }).catch((reason: unknown) => {
      if (active) reportSetupError(reason)
    })
    void getMicrophones().then((next) => {
      if (active) setMicrophones(next)
    }).catch((reason: unknown) => {
      if (active) reportSetupError(reason)
    })
    return () => {
      active = false
      micTestVersion.current += 1
    }
  }, [reportSetupError])
  const handleSetupEvent = useCallback((event: SetupEvent) => {
    if (event.kind === 'progress') {
      setReadiness((current) => current && applySetupProgress(current, event))
    }
    if (event.kind === 'failed') setSetupError(event.error)
  }, [])
  const getSetupRefresh = useCallback((event: SetupEvent) => {
    if (event.kind === 'progress') return null
    return () => getReadiness().then((next) => () => setReadiness(next))
  }, [])
  useAsyncSubscription({
    subscribe: onSetupEvent,
    onEvent: handleSetupEvent,
    getRefresh: getSetupRefresh,
    onError: reportSetupError,
  })
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
                .catch(reportSetupError)
            }}
            onSelect={(id) => {
              micTestVersion.current += 1
              setMicTest(null)
              void setMicrophone(id)
                .then((nextMicrophones) => {
                  setMicrophones(nextMicrophones)
                  return getReadiness()
                })
                .then(setReadiness)
                .catch(reportSetupError)
            }}
            onTest={(id, fallback) => {
              const version = ++micTestVersion.current
              setTestingMic(true)
              const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
              void run
                .then((result) => {
                  if (micTestVersion.current !== version) return null
                  setMicTest(result)
                  return getReadiness()
                })
                .then((next) => {
                  if (next && micTestVersion.current === version) setReadiness(next)
                })
                .catch((reason: unknown) => {
                  if (micTestVersion.current === version) reportSetupError(reason)
                })
                .finally(() => {
                  if (micTestVersion.current === version) setTestingMic(false)
                })
            }}
          />
        </div>
      ) : null}
      {readiness && !readiness.speechReady ? (
        <div className="first-run-step">
          <strong>2 · Set up local speech</strong>
          <SpeechSetupSection
            readiness={readiness}
            guided
            onRefresh={() => void getReadiness().then(setReadiness).catch(reportSetupError)}
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
