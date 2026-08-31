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

  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
    }
  }, [])

  const refresh = useCallback(async () => {
    try {
      const next = await getDictionary()
      if (active.current) setItems(next)
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }, [onError])

  useEffect(() => {
    let current = true
    void getDictionary().then((next) => {
      if (current && active.current) setItems(next)
    }).catch((reason: unknown) => {
      if (current && active.current) onError(reason)
    })
    return () => {
      current = false
    }
  }, [onError])

  const add = useCallback(async (spoken: string, written: string) => {
    try {
      await addDictionaryEntry(spoken, written)
      await refresh()
    } catch (reason) {
      if (active.current) onError(reason)
      throw reason
    }
  }, [onError, refresh])

  const remove = useCallback(async (entry: DictionaryItem) => {
    try {
      const removed = await removeDictionaryEntry(entry.spoken, entry.written)
      if (!removed && active.current) onError(`"${entry.spoken}" was already removed.`)
      await refresh()
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }, [onError, refresh])

  const addBatch = useCallback(async (written: string, spoken: string[]) => {
    try {
      const result = await addDictionaryEntriesBatch(written, spoken)
      if (active.current) setItems(result.entries)
      return result
    } catch (reason) {
      if (active.current) onError(reason)
      throw reason
    }
  }, [onError])

  return { items, add, addBatch, remove, refresh }
}
