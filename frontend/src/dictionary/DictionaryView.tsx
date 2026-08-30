import { Plus, Trash2 } from 'lucide-react'
import { FormEvent, useState } from 'react'

import { BarsMotif, ViewHeader } from '../app/chrome'
import type { DictionaryItem } from '../generated/ipc'

export function DictionaryView({
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
