import { Check, Clock3, Copy, Search, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { BarsMotif, ViewHeader } from '../app/chrome'
import { formatDateTime, messageFrom } from '../app/formatting'
import { groupByDay, millisecondsUntilNextLocalDay } from '../stats'
import { copyText } from '../tauri'
import type { HistoryItem } from '../generated/ipc'

export function HistoryView({
  items,
  onDelete,
  onClear,
  onError,
}: {
  items: HistoryItem[]
  onDelete: (id: string) => Promise<boolean>
  onClear: () => Promise<number>
  onError: (message: string) => void
}) {
  const [query, setQuery] = useState('')
  const [pendingId, setPendingId] = useState<string | null>(null)
  const [clearing, setClearing] = useState(false)
  const [calendarDate, setCalendarDate] = useState(() => new Date())
  useEffect(() => {
    const timer = window.setTimeout(
      () => setCalendarDate(new Date()),
      millisecondsUntilNextLocalDay(calendarDate),
    )
    return () => window.clearTimeout(timer)
  }, [calendarDate])
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    return needle ? items.filter((item) => item.text.toLocaleLowerCase().includes(needle)) : items
  }, [items, query])
  const groups = useMemo(() => groupByDay(filtered, calendarDate), [filtered, calendarDate])
  const busy = clearing || pendingId !== null
  const remove = async (item: HistoryItem) => {
    if (busy || !window.confirm('Delete this transcript permanently? This action cannot be undone.')) return
    setPendingId(item.id)
    try {
      await onDelete(item.id)
    } finally {
      setPendingId(null)
    }
  }
  const clear = async () => {
    if (busy || !window.confirm('Clear all saved history permanently? This action cannot be undone.')) return
    setClearing(true)
    try {
      await onClear()
    } finally {
      setClearing(false)
    }
  }
  return (
    <div className="view-stack">
      <ViewHeader title="History" subtitle="Every successful local transcription, newest first." />
      <div className="history-toolbar">
        <label className="search-field">
          <Search size={17} aria-hidden="true" />
          <span className="sr-only">Search history</span>
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search transcripts…" />
        </label>
        {items.length > 0 ? (
          <button
            className="secondary-button compact-button danger-button history-clear-button"
            type="button"
            disabled={busy}
            onClick={() => void clear()}
          >
            <Trash2 size={16} aria-hidden="true" />
            Clear all history
          </button>
        ) : null}
      </div>
      {groups.map((group) => (
        <section className="panel transcript-list" aria-live="polite" key={group.label}>
          <h3 className="day-header">{group.label}</h3>
          {group.items.map((item) => (
            <TranscriptRow
              key={item.id}
              item={item}
              deleting={pendingId === item.id}
              disabled={busy}
              onDelete={remove}
              onError={onError}
            />
          ))}
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

function TranscriptRow({
  item,
  deleting,
  disabled,
  onDelete,
  onError,
}: {
  item: HistoryItem
  deleting: boolean
  disabled: boolean
  onDelete: (item: HistoryItem) => Promise<void>
  onError: (message: string) => void
}) {
  const [copied, setCopied] = useState(false)
  const mountedRef = useRef(true)
  const copyVersionRef = useRef(0)
  const feedbackTimeoutRef = useRef<number | null>(null)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      copyVersionRef.current += 1
      if (feedbackTimeoutRef.current !== null) {
        window.clearTimeout(feedbackTimeoutRef.current)
        feedbackTimeoutRef.current = null
      }
    }
  }, [])

  const copy = async () => {
    if (!mountedRef.current) return
    const version = ++copyVersionRef.current
    if (feedbackTimeoutRef.current !== null) {
      window.clearTimeout(feedbackTimeoutRef.current)
      feedbackTimeoutRef.current = null
    }
    setCopied(false)
    try {
      await copyText(item.text)
      if (!mountedRef.current || copyVersionRef.current !== version) return
      setCopied(true)
      feedbackTimeoutRef.current = window.setTimeout(() => {
        feedbackTimeoutRef.current = null
        if (mountedRef.current && copyVersionRef.current === version) setCopied(false)
      }, 1200)
    } catch (reason) {
      if (mountedRef.current && copyVersionRef.current === version) onError(messageFrom(reason))
    }
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
      <div className="transcript-actions">
        <button className="icon-button" type="button" onClick={() => void copy()} aria-label={copied ? 'Copied transcript' : 'Copy transcript'}>
          {copied ? <Check size={17} aria-hidden="true" /> : <Copy size={17} aria-hidden="true" />}
        </button>
        <button
          className="icon-button danger-button"
          type="button"
          disabled={disabled}
          onClick={() => void onDelete(item)}
          aria-label={`Delete transcript: ${item.text}`}
        >
          <Trash2 size={17} aria-hidden="true" />
          {deleting ? <span className="sr-only">Deleting</span> : null}
        </button>
      </div>
    </article>
  )
}
