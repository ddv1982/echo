import { useEffect } from 'react'

type Commit = () => void

interface AsyncSubscriptionOptions<T> {
  subscribe: (handler: (event: T) => void) => Promise<() => void>
  onEvent: (event: T) => void | Promise<Commit>
  onError?: (reason: unknown) => void
}

export function useAsyncSubscription<T>({
  subscribe,
  onEvent,
  onError,
}: AsyncSubscriptionOptions<T>) {
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | null = null
    let inFlight: Promise<void> | null = null
    let queued: { event: T } | null = null

    const dispatch = (event: T) => {
      if (!active) return
      if (inFlight) {
        queued = { event }
        return
      }
      try {
        const pending = onEvent(event)
        if (!pending) return
        inFlight = pending.then((commit) => {
          if (active) commit()
        }).catch((reason: unknown) => {
          if (active) onError?.(reason)
        }).finally(() => {
          inFlight = null
          if (!active || !queued) return
          const next = queued.event
          queued = null
          dispatch(next)
        })
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
  }, [onError, onEvent, subscribe])
}
