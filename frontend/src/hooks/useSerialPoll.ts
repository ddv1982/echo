import { useCallback, useEffect, useRef } from 'react'

interface SerialPollOptions<T> {
  request: () => Promise<T>
  onResult: (result: T) => void
  onError?: (reason: unknown) => void
  intervalMs: number
  enabled?: boolean
  shouldPoll?: () => boolean
}

const settled = Promise.resolve()

export function useSerialPoll<T>({
  request,
  onResult,
  onError,
  intervalMs,
  enabled = true,
  shouldPoll,
}: SerialPollOptions<T>): () => Promise<void> {
  const triggerRef = useRef<() => Promise<void>>(() => settled)

  useEffect(() => {
    if (!enabled) {
      triggerRef.current = () => settled
      return
    }

    let disposed = false
    let timer: number | null = null
    let inFlight: Promise<void> | null = null
    let queued: Promise<void> | null = null

    const clearTimer = () => {
      if (timer == null) return
      window.clearTimeout(timer)
      timer = null
    }

    const schedule = () => {
      clearTimer()
      timer = window.setTimeout(() => void run(false), intervalMs)
    }

    const requestOnce = () => {
      const current = (async () => {
        try {
          const result = await request()
          if (!disposed) onResult(result)
        } catch (reason) {
          if (!disposed) onError?.(reason)
        } finally {
          inFlight = null
          if (!disposed) schedule()
        }
      })()
      inFlight = current
      return current
    }

    async function run(manual: boolean) {
      clearTimer()
      if (disposed) return
      if (!manual && shouldPoll && !shouldPoll()) {
        schedule()
        return
      }
      if (!inFlight) {
        await requestOnce()
        return
      }
      if (!manual) return inFlight
      if (!queued) {
        queued = (async () => {
          await inFlight
          if (disposed) return
          clearTimer()
          await requestOnce()
        })().finally(() => {
          queued = null
        })
      }
      await queued
    }

    const trigger = () => run(true)
    triggerRef.current = trigger
    timer = window.setTimeout(() => void run(false), 0)

    return () => {
      disposed = true
      clearTimer()
      if (triggerRef.current === trigger) triggerRef.current = () => settled
    }
  }, [enabled, intervalMs, onError, onResult, request, shouldPoll])

  return useCallback(() => triggerRef.current(), [])
}
