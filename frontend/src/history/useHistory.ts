import { useCallback, useEffect, useRef, useState } from 'react'

import { getHistory } from '../tauri'
import type { HistoryItem } from '../generated/ipc'

export function useHistory(onError: (reason: unknown) => void) {
  const [items, setItems] = useState<HistoryItem[]>([])
  const active = useRef(true)

  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
    }
  }, [])

  const refresh = useCallback(async () => {
    try {
      const next = await getHistory()
      if (active.current) setItems(next)
    } catch (reason) {
      if (active.current) onError(reason)
    }
  }, [onError])

  useEffect(() => {
    let current = true
    void getHistory().then((next) => {
      if (current && active.current) setItems(next)
    }).catch((reason: unknown) => {
      if (current && active.current) onError(reason)
    })
    return () => {
      current = false
    }
  }, [onError])

  return { items, refresh }
}
