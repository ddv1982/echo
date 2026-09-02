import { useCallback, useEffect, useRef, useState } from 'react'

import { clearHistory, deleteHistoryItem, getHistory } from '../tauri'
import type { HistoryItem } from '../generated/ipc'

export function useHistory(onError: (reason: unknown) => void) {
  const [items, setItems] = useState<HistoryItem[]>([])
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
      const next = await getHistory()
      if (active.current) setItems(next)
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }), [enqueue, onError])

  useEffect(() => {
    void refresh().catch(onError)
  }, [onError, refresh])

  const remove = useCallback((id: string) => enqueue(async () => {
    try {
      const removed = await deleteHistoryItem(id)
      if (active.current) {
        setItems((current) => current.filter((item) => item.id !== id))
      }
      return removed
    } catch (reason) {
      if (active.current) onError(reason)
      return false
    }
  }), [enqueue, onError])

  const clear = useCallback(() => enqueue(async () => {
    try {
      const count = await clearHistory()
      if (active.current) setItems([])
      return count
    } catch (reason) {
      if (active.current) onError(reason)
      return 0
    }
  }), [enqueue, onError])

  return { items, remove, clear, refresh }
}
