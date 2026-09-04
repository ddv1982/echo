import { useEffect } from 'react'

type Commit = () => void | Promise<void>

interface AsyncSubscriptionOptions<T> {
  subscribe: (handler: (event: T) => void) => Promise<() => void>
  onEvent: (event: T) => void
  getRefresh: (event: T) => (() => Promise<Commit>) | null
  initialRefresh?: () => Promise<Commit>
  onError?: (reason: unknown) => void
}

export function useAsyncSubscription<T>({
  subscribe,
  onEvent,
  getRefresh,
  initialRefresh,
  onError,
}: AsyncSubscriptionOptions<T>) {
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | null = null
    let inFlight: Promise<void> | null = null
    const queued: Array<() => Promise<Commit>> = []

    const runRefresh = (refresh: () => Promise<Commit>) => {
      inFlight = Promise.resolve().then(refresh).then((commit) => {
        if (active) return commit()
      }).catch((reason: unknown) => {
        if (active) onError?.(reason)
      }).finally(() => {
        inFlight = null
        if (!active) return
        const next = queued.shift()
        if (next) runRefresh(next)
      })
    }

    const scheduleRefresh = (refresh: () => Promise<Commit>) => {
      if (inFlight) {
        queued.push(refresh)
      } else {
        runRefresh(refresh)
      }
    }

    const dispatch = (event: T) => {
      if (!active) return
      try {
        onEvent(event)
        const refresh = getRefresh(event)
        if (!refresh) return
        scheduleRefresh(refresh)
      } catch (reason) {
        if (active) onError?.(reason)
      }
    }

    void subscribe(dispatch).then((next) => {
      if (active) {
        unlisten = next
        if (initialRefresh) scheduleRefresh(initialRefresh)
      } else {
        next()
      }
    }).catch((reason: unknown) => {
      if (!active) return
      onError?.(reason)
      if (initialRefresh) scheduleRefresh(initialRefresh)
    })

    return () => {
      active = false
      queued.length = 0
      unlisten?.()
    }
  }, [getRefresh, initialRefresh, onError, onEvent, subscribe])
}
