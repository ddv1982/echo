import { Check, Clock3, Copy, Search, Trash2 } from 'lucide-react'
import { useMemo, useState } from 'react'

import { BarsMotif, ViewHeader } from '../app/chrome'
import { formatDateTime } from '../app/formatting'
import { groupByDay } from '../stats'
import { copyText } from '../tauri'
import type { HistoryItem } from '../generated/ipc'

export function HistoryView({
  items,
  onDelete,
  onClear,
}: {
  items: HistoryItem[]
  onDelete: (id: string) => Promise<boolean>
  onClear: () => Promise<number>
}) {
  const [query, setQuery] = useState('')
  const [pendingId, setPendingId] = useState<string | null>(null)
  const [clearing, setClearing] = useState(false)
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    return needle ? items.filter((item) => item.text.toLocaleLowerCase().includes(needle)) : items
  }, [items, query])
  const groups = useMemo(() => groupByDay(filtered, new Date()), [filtered])
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
}: {
  item: HistoryItem
  deleting: boolean
  disabled: boolean
  onDelete: (item: HistoryItem) => Promise<void>
}) {
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
      <div className="transcript-actions">
        <button className="icon-button" type="button" onClick={() => void copy()} aria-label="Copy transcript">
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
