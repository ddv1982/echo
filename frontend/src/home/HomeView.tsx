import { ArrowUpRight, CircleAlert, LoaderCircle, Mic, Square } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { BarsMotif, SectionHeading } from '../app/chrome'
import { formatDuration, formatTime, messageFrom } from '../app/formatting'
import { useSerialPoll } from '../hooks/useSerialPoll'
import { presentShortcut } from '../shortcut'
import { deriveStats, millisecondsUntilNextLocalDay } from '../stats'
import { getRecordingLevel, removeStaleInstalls } from '../tauri'
import type { AppPhase, AppStatus, HistoryItem } from '../generated/ipc'
import { SetupChecklist } from './SetupChecklist'

const phasePresentation = {
  Idle: { title: 'Ready when you are', action: 'Start recording' },
  Recording: { title: 'Listening…', action: 'Stop and transcribe' },
  Transcribing: { title: 'Transcribing locally…', action: 'Transcribing' },
  Injecting: { title: 'Inserting your text…', action: 'Inserting text' },
  Failed: { title: 'Let’s try that again', action: 'Start recording' },
} satisfies Record<AppPhase, { title: string; action: string }>

export function HomeView({
  status,
  history,
  recordingSeconds,
  onToggleRecording,
  onOpenSettings,
  onOpenHistory,
}: {
  status: AppStatus
  history: HistoryItem[]
  recordingSeconds: number
  onToggleRecording: () => Promise<void>
  onOpenSettings: () => void
  onOpenHistory: () => void
}) {
  const shortcut = presentShortcut(status.shortcut)
  const recording = status.phase === 'Recording'
  const processing = status.phase === 'Transcribing' || status.phase === 'Injecting'
  const presentation = phasePresentation[status.phase]
  const description = recording
    ? 'Speak naturally. Stop when you’re done.'
    : status.phase === 'Transcribing'
      ? `${status.engineName} is turning your recording into text.`
      : status.phase === 'Injecting'
        ? 'Sending the transcript to your active cursor.'
        : status.phase === 'Failed'
          ? 'Check the error details, then start a new recording.'
          : 'Speak your mind. Your audio stays on this machine.'
  return (
    <div className="view-stack">
      <section className="record-hero" data-state={status.phase.toLowerCase()} aria-label="Dictation">
        <div className="hero-copy" aria-live="polite" aria-atomic="true">
          <span className="eyebrow">Your words, without the typing</span>
          <h2>{presentation.title}</h2>
          <p>{description}</p>
        </div>
        <div className="record-actions">
          <button
            className="record-button"
            type="button"
            onClick={() => void onToggleRecording()}
            disabled={processing}
          >
            {recording ? <Square size={17} fill="currentColor" aria-hidden="true" />
              : processing ? <LoaderCircle className="processing-icon" size={18} aria-hidden="true" />
                : <Mic size={18} aria-hidden="true" />}
            <span>{presentation.action}</span>
          </button>
          <div className="shortcut-hint">
            <kbd>{shortcut.display}</kbd>
            <span>{shortcut.ready ? 'from any app' : shortcut.manualCommand ? 'Bind it in your desktop settings.' : 'setup required in Settings'}</span>
          </div>
          {recording ? (
            <span className="readout-timer">
              {status.recordingLimitSeconds == null
                ? formatDuration(recordingSeconds)
                : `${formatDuration(Math.min(recordingSeconds, status.recordingLimitSeconds))} / ${formatDuration(status.recordingLimitSeconds)}`}
            </span>
          ) : null}
        </div>
        {recording ? <LevelBars live={status.recordingInProcess} /> : null}
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

      <div className="home-grid">
        <section className="panel last-transcript">
          <SectionHeading title="Last transcript" subtitle="Your most recent dictation" />
          {status.lastTranscript ? (
            <blockquote>{status.lastTranscript}</blockquote>
          ) : (
            <div className="empty-state compact">
              <BarsMotif />
              <span>Your next transcript will appear here.</span>
            </div>
          )}
        </section>
        <section className="recent-panel">
          <div className="recent-heading">
            <SectionHeading title="Recent" subtitle={`${history.length} saved transcript${history.length === 1 ? '' : 's'}`} />
            <button type="button" className="text-button" onClick={onOpenHistory}>View history <ArrowUpRight size={14} aria-hidden="true" /></button>
          </div>
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
      <StatsStrip history={history} />
    </div>
  )
}

const LEVEL_BAR_COUNT = 14

function LevelBars({ live }: { live: boolean }) {
  const [levels, setLevels] = useState<number[]>(() =>
    Array.from({ length: LEVEL_BAR_COUNT }, () => 0))
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
  const [calendarDate, setCalendarDate] = useState(() => new Date())
  useEffect(() => {
    const timer = window.setTimeout(
      () => setCalendarDate(new Date()),
      millisecondsUntilNextLocalDay(calendarDate),
    )
    return () => window.clearTimeout(timer)
  }, [calendarDate])
  const stats = useMemo(() => deriveStats(history, calendarDate), [history, calendarDate])
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
