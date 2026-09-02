import process from 'node:process'
import { setImmediate } from 'node:timers/promises'
import { describe, expect, it, vi } from 'vitest'
import { startStatusPerfProbe } from './startStatusPerfProbe'

interface LoadedStatusPerf {
  startStatusPerf: () => void
}

type StatusPerfLoader = () => Promise<LoadedStatusPerf>
type ErrorReporter = (reason: unknown) => void

describe('startStatusPerfProbe', () => {
  it('reports a rejected loader exactly once without an unhandled rejection', async () => {
    const loadError = new Error('status performance module failed to load')
    const loader = vi.fn<StatusPerfLoader>().mockRejectedValue(loadError)
    const reportError = vi.fn<ErrorReporter>()
    const unhandledReasons: unknown[] = []
    const onUnhandledRejection = (reason: unknown) => {
      unhandledReasons.push(reason)
    }
    process.on('unhandledRejection', onUnhandledRejection)

    try {
      const result = startStatusPerfProbe(loader, reportError)
      expect(result).toBeUndefined()

      await Promise.resolve()
      await setImmediate()
      await Promise.resolve()

      expect(loader).toHaveBeenCalledTimes(1)
      expect(reportError).toHaveBeenCalledTimes(1)
      expect(reportError).toHaveBeenCalledWith(loadError)
      expect(unhandledReasons).toEqual([])
    } finally {
      process.off('unhandledRejection', onUnhandledRejection)
    }
  })

  it('starts the loaded probe without reporting an error', async () => {
    const startStatusPerf = vi.fn<() => void>()
    const loader = vi.fn<StatusPerfLoader>().mockResolvedValue({ startStatusPerf })
    const reportError = vi.fn<ErrorReporter>()

    const result = startStatusPerfProbe(loader, reportError)
    expect(result).toBeUndefined()

    await setImmediate()

    expect(loader).toHaveBeenCalledTimes(1)
    expect(startStatusPerf).toHaveBeenCalledTimes(1)
    expect(reportError).not.toHaveBeenCalled()
  })
})
