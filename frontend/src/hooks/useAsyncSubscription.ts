import { useEffect } from 'react'

type Commit = () => void

interface AsyncSubscriptionOptions<T> {
  subscribe: (handler: (event: T) => void) => Promise<() => void>
  onEvent: (event: T) => void
  getRefresh: (event: T) => (() => Promise<Commit>) | null
  onError?: (reason: unknown) => void
}

export function useAsyncSubscription<T>({
  subscribe,
  onEvent,
  getRefresh,
  onError,
}: AsyncSubscriptionOptions<T>) {
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | null = null
    let inFlight: Promise<void> | null = null
    let queued: (() => Promise<Commit>) | null = null

    const runRefresh = (refresh: () => Promise<Commit>) => {
      inFlight = Promise.resolve().then(refresh).then((commit) => {
        if (active) commit()
      }).catch((reason: unknown) => {
        if (active) onError?.(reason)
      }).finally(() => {
        inFlight = null
        if (!active || !queued) return
        const next = queued
        queued = null
        runRefresh(next)
      })
    }

    const dispatch = (event: T) => {
      if (!active) return
      try {
        onEvent(event)
        const refresh = getRefresh(event)
        if (!refresh) return
        if (inFlight) {
          queued = refresh
        } else {
          runRefresh(refresh)
        }
      } catch (reason) {
        if (active) onError?.(reason)
      }
    }

    void subscribe(dispatch).then((next) => {
      if (active) {
        unlisten = next
      } else {
        next()
      }
    }).catch((reason: unknown) => {
      if (active) onError?.(reason)
    })

    return () => {
      active = false
      unlisten?.()
    }
  }, [getRefresh, onError, onEvent, subscribe])
}
