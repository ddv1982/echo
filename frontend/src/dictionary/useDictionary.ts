import { useCallback, useEffect, useRef, useState } from 'react'

import {
  addDictionaryEntriesBatch,
  addDictionaryEntry,
  getDictionary,
  removeDictionaryEntry,
} from '../tauri'
import type { DictionaryItem } from '../generated/ipc'

export function useDictionary(onError: (reason: unknown) => void) {
  const [items, setItems] = useState<DictionaryItem[]>([])
  const active = useRef(true)
  const operations = useRef<Promise<void>>(Promise.resolve())

  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
    }
  }, [])

  const enqueue = useCallback(<T,>(operation: () => Promise<T>) => {
    const result = operations.current.then(operation)
    operations.current = result.then(() => undefined, () => undefined)
    return result
  }, [])

  const refresh = useCallback(() => enqueue(async () => {
    try {
      const next = await getDictionary()
      if (active.current) setItems(next)
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }), [enqueue, onError])

  useEffect(() => {
    void refresh().catch(onError)
  }, [onError, refresh])

  const add = useCallback((spoken: string, written: string) => enqueue(async () => {
    await addDictionaryEntry(spoken, written)
    try {
      const next = await getDictionary()
      if (active.current) setItems(next)
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }), [enqueue, onError])

  const remove = useCallback((entry: DictionaryItem) => enqueue(async () => {
    try {
      const removed = await removeDictionaryEntry(entry.spoken, entry.written)
      if (!removed && active.current) onError(`"${entry.spoken}" was already removed.`)
      try {
        const next = await getDictionary()
        if (active.current) setItems(next)
      } catch (reason) {
        if (active.current) onError(reason)
      }
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }), [enqueue, onError])

  const addBatch = useCallback((written: string, spoken: string[]) => enqueue(async () => {
    const result = await addDictionaryEntriesBatch(written, spoken)
    if (active.current) setItems(result.entries)
    return result
  }), [enqueue])

  return { items, add, addBatch, remove, refresh }
}
