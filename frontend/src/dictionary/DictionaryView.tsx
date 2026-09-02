import { Mic, Plus, Trash2 } from 'lucide-react'
import { useRef, useState } from 'react'
import type { SubmitEvent } from 'react'

import { BarsMotif, ViewHeader } from '../app/chrome'
import { messageFrom } from '../app/formatting'
import type { DictionaryBatchResult, DictionaryItem } from '../generated/ipc'
import { DictionaryTrainer } from './DictionaryTrainer'

export function DictionaryView({
  items,
  onAdd,
  onAddBatch,
  onRemove,
  onError,
}: {
  items: DictionaryItem[]
  onAdd: (spoken: string, written: string) => Promise<void>
  onAddBatch: (written: string, spoken: string[]) => Promise<DictionaryBatchResult>
  onRemove: (item: DictionaryItem) => Promise<void>
  onError: (message: string) => void
}) {
  const [spoken, setSpoken] = useState('')
  const [written, setWritten] = useState('')
  const [saving, setSaving] = useState(false)
  const [trainerOpen, setTrainerOpen] = useState(false)
  const trainerTriggerRef = useRef<HTMLButtonElement>(null)
  const submit = async (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!spoken.trim() || !written.trim()) return
    setSaving(true)
    try {
      await onAdd(spoken, written)
      setSpoken('')
      setWritten('')
    } catch (reason) {
      onError(messageFrom(reason))
    } finally {
      setSaving(false)
    }
  }
  return (
    <div className="view-stack">
      <ViewHeader title="Dictionary" subtitle="Teach Echo names, products, and phrases your transcription model often mishears." />
      <form className="panel dictionary-form" onSubmit={(event) => void submit(event)}>
        <label><span>What Echo hears</span><input value={spoken} onChange={(event) => setSpoken(event.target.value)} placeholder="clawed code" /></label>
        <div className="mapping-arrow" aria-hidden="true">→</div>
        <label><span>What Echo should write</span><input value={written} onChange={(event) => setWritten(event.target.value)} placeholder="Claude Code" /></label>
        <button className="primary-button compact-button" type="submit" disabled={saving || !spoken.trim() || !written.trim()}><Plus size={17} /> Add</button>
      </form>
      <div className="dictionary-training-prompt">
        <div><strong>Not sure what Echo hears?</strong><span>Say the phrase five times and review the pronunciations together.</span></div>
        <button ref={trainerTriggerRef} className="secondary-button" type="button" onClick={() => setTrainerOpen(true)}>
          <Mic size={16} aria-hidden="true" /> Teach by voice
        </button>
      </div>
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
      {trainerOpen ? (
        <DictionaryTrainer
          items={items}
          triggerRef={trainerTriggerRef}
          onClose={() => setTrainerOpen(false)}
          onSave={onAddBatch}
        />
      ) : null}
    </div>
  )
}
