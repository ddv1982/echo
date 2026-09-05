import { invoke } from '@tauri-apps/api/core'

export interface SampleSummary {
  count: number
  minMs: number
  p50Ms: number
  p95Ms: number
  maxMs: number
}

interface PerfLane {
  name: string
  summary: SampleSummary
  samplesMs: number[]
}

export interface NativeVerificationReport {
  checks: { name: string; passed: boolean }[]
  timingsMs: Record<string, number>
  settingsRevisions: number[]
}

interface PerfReport {
  schemaVersion: 1
  appVersion: string
  userAgent: string
  platform: string
  lanes: PerfLane[]
  verification?: NativeVerificationReport
}

function percentile(sorted: readonly number[], fraction: number): number {
  const rank = (sorted.length - 1) * fraction
  const lower = Math.floor(rank)
  const upper = Math.ceil(rank)
  const lowerValue = sorted[lower]
  const upperValue = sorted[upper]
  if (lowerValue === undefined || upperValue === undefined) {
    throw new Error('percentile index is outside the sample set')
  }
  return lowerValue + (upperValue - lowerValue) * (rank - lower)
}

export function summarizeSamples(samples: readonly number[]): SampleSummary {
  if (samples.length === 0) throw new Error('sample set is empty')
  if (samples.some((sample) => !Number.isFinite(sample) || sample < 0)) {
    throw new Error('sample set contains an invalid duration')
  }
  const sorted = [...samples].sort((left, right) => left - right)
  return {
    count: sorted.length,
    minMs: sorted[0] ?? 0,
    p50Ms: percentile(sorted, 0.5),
    p95Ms: percentile(sorted, 0.95),
    maxMs: sorted[sorted.length - 1] ?? 0,
  }
}

async function measure(
  name: string,
  command: string,
  warmups: number,
  samples: number,
): Promise<PerfLane> {
  for (let index = 0; index < warmups; index += 1) await invoke(command)
  const samplesMs: number[] = []
  for (let index = 0; index < samples; index += 1) {
    const started = performance.now()
    await invoke(command)
    samplesMs.push(performance.now() - started)
  }
  return { name, summary: summarizeSamples(samplesMs), samplesMs }
}

async function runStatusPerf(verification?: NativeVerificationReport): Promise<void> {
  const noop = await measure('noop', 'perf_noop', 20, 500)
  await invoke('perf_clear_status_stages')
  const fixedStatus = await measure('fixed-status', 'perf_fixed_status', 20, 500)
  await invoke('perf_preserve_cold_status_stage')
  for (let index = 0; index < 3; index += 1) await invoke('get_app_status')
  await invoke('perf_clear_status_stages')
  const currentStatus = await measure('current-status', 'get_app_status', 0, 40)
  const report = {
    schemaVersion: 1,
    appVersion: __APP_VERSION__,
    userAgent: navigator.userAgent,
    platform: navigator.platform,
    lanes: [noop, fixedStatus, currentStatus],
    ...(verification === undefined ? {} : { verification }),
  } satisfies PerfReport
  await invoke('perf_report_complete', { report })
}

export function startStatusPerf(verification?: NativeVerificationReport): void {
  runStatusPerf(verification).catch((reason: unknown) => {
    const message = reason instanceof Error ? reason.message : String(reason)
    invoke('perf_report_failed', { message }).catch((reportingError: unknown) => {
      console.error('Failed to report status performance failure:', reportingError)
    })
  })
}
