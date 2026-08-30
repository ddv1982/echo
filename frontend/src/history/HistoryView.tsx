import { Check, Clock3, Copy, Search } from 'lucide-react'
import { useMemo, useState } from 'react'

import { BarsMotif, ViewHeader } from '../app/chrome'
import { formatDateTime } from '../app/formatting'
import { groupByDay } from '../stats'
import { copyText } from '../tauri'
import type { HistoryItem } from '../generated/ipc'

export function HistoryView({ items }: { items: HistoryItem[] }) {
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
