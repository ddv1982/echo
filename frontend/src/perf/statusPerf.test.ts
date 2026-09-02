import process from 'node:process'
import { setImmediate } from 'node:timers/promises'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { startStatusPerf, summarizeSamples } from './statusPerf'

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn<(command: string, commandArguments?: unknown) => Promise<unknown>>(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

beforeEach(() => {
  invokeMock.mockReset()
})

describe('summarizeSamples', () => {
  it('reports interpolated percentiles without changing the input order', () => {
    const samples = [4, 1, 3, 2]

    const summary = summarizeSamples(samples)
    expect(summary).toMatchObject({
      count: 4,
      minMs: 1,
      p50Ms: 2.5,
      maxMs: 4,
    })
    expect(summary.p95Ms).toBeCloseTo(3.85)
    expect(samples).toEqual([4, 1, 3, 2])
  })

  it('rejects an empty sample set', () => {
    expect(() => summarizeSamples([])).toThrow('sample set is empty')
  })
})

describe('startStatusPerf', () => {
  it('reports the first benchmark failure with its primary error message', async () => {
    const primaryError = new Error('first benchmark failed')
    invokeMock.mockRejectedValueOnce(primaryError).mockResolvedValueOnce(undefined)

    startStatusPerf()

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledTimes(2)
    })
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'perf_noop')
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'perf_report_failed', {
      message: primaryError.message,
    })
  })

  it('terminally handles rejection while reporting a benchmark failure', async () => {
    const primaryError = new Error('first benchmark failed')
    const reportingError = new Error('failure report rejected')
    const unhandledReasons: unknown[] = []
    const onUnhandledRejection = (reason: unknown) => {
      unhandledReasons.push(reason)
    }
    const consoleErrorMock = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    invokeMock.mockRejectedValueOnce(primaryError).mockRejectedValueOnce(reportingError)
    process.on('unhandledRejection', onUnhandledRejection)

    try {
      startStatusPerf()
      await Promise.resolve()
      await setImmediate()
      await Promise.resolve()

      expect(invokeMock).toHaveBeenCalledTimes(2)
      expect(invokeMock).toHaveBeenNthCalledWith(2, 'perf_report_failed', {
        message: primaryError.message,
      })
      expect(consoleErrorMock).toHaveBeenCalledTimes(1)
      expect(consoleErrorMock.mock.calls.flat()).toContain(reportingError)
      expect(unhandledReasons).toEqual([])
    } finally {
      process.off('unhandledRejection', onUnhandledRejection)
      consoleErrorMock.mockRestore()
    }
  })
})
