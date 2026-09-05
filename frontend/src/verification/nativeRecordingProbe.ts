import { invoke } from '@tauri-apps/api/core'

import { tauriDesktopApi as api } from '../api/tauriDesktopApi'
import { startStatusPerf } from '../perf/statusPerf'

interface Check {
  name: string
  passed: boolean
}

interface VerificationReport {
  checks: Check[]
  timingsMs: Record<string, number>
  settingsRevisions: number[]
}

function assert(value: boolean, message: string): asserts value {
  if (!value) throw new Error(message)
}

function elapsedSince(started: number): number {
  return performance.now() - started
}

function addCheck(checks: Check[], name: string, value: boolean): void {
  checks.push({ name, passed: value })
  assert(value, name)
}

async function waitForTerminal(sessionId: string, deadlineMs: number): Promise<Awaited<ReturnType<typeof api.getAppStatus>>> {
  const deadline = performance.now() + deadlineMs
  let status = await api.getAppStatus()
  while (status.phase !== 'Idle' && status.phase !== 'Failed' && performance.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 20))
    status = await api.getAppStatus()
  }
  assert(status.recordingSessionId === null || status.recordingSessionId === sessionId, 'terminal status belongs to the recording')
  return status
}

async function verify(): Promise<void> {
  const checks: Check[] = []
  const timingsMs: Record<string, number> = {}

  const initial = await api.getSettings()
  const first = api.setSettings({ kind: 'hud', value: false })
  const microphone = api.setMicrophone(null)
  const second = api.setSettings({ kind: 'hud', value: true })
  const [firstSaved, microphoneSaved, secondSaved] = await Promise.all([first, microphone, second])
  const revisions = [initial.revision, firstSaved.revision, microphoneSaved.revision, secondSaved.revision]
  addCheck(checks, 'settings mutations retain their own saved values', firstSaved.preferences.hud.value === false && secondSaved.preferences.hud.value === true)
  addCheck(checks, 'settings and microphone mutations are FIFO', revisions.every((value, index) => index === 0 || value > (revisions[index - 1] ?? value)))
  let invalidRejected = false
  try {
    await api.setSettings({ kind: 'language', value: 'invalid-language' })
  } catch {
    invalidRejected = true
  }
  addCheck(checks, 'invalid setting rejects through its completion channel', invalidRejected)
  const afterInvalid = await api.getSettings()
  addCheck(checks, 'settings queue recovers after rejection', afterInvalid.preferences.hud.value === true && afterInvalid.revision > secondSaved.revision)

  const startRequestedAt = performance.now()
  const started = await api.startCapture()
  timingsMs.startReceipt = elapsedSince(startRequestedAt)
  addCheck(checks, 'start receipt identifies the active recording', started.sessionId !== null && started.phase === 'Recording')
  const sessionId = started.sessionId
  assert(sessionId !== null, 'recording session ID is missing')

  const staleRequestedAt = performance.now()
  let staleRejected = false
  try {
    await api.stopCapture(`${sessionId}-stale`)
  } catch {
    staleRejected = true
  }
  timingsMs.staleStopReceipt = elapsedSince(staleRequestedAt)
  addCheck(checks, 'stale stop is rejected', staleRejected)
  const live = await api.getAppStatus()
  addCheck(checks, 'stale stop leaves the active recording unchanged', live.recordingSessionId === sessionId && !live.captureStopRequested)

  await new Promise((resolve) => setTimeout(resolve, 100))
  const stopRequestedAt = performance.now()
  const stopped = await api.stopCapture(sessionId)
  timingsMs.stopReceipt = elapsedSince(stopRequestedAt)
  addCheck(checks, 'normal stop receipt retains its session identity', stopped.sessionId === sessionId)
  const terminal = await waitForTerminal(sessionId, 10_000)
  timingsMs.terminalObservation = elapsedSince(stopRequestedAt)
  addCheck(checks, 'normal stop reaches an idle terminal observation with History', terminal.phase === 'Idle' && terminal.lastHistoryId !== null)
  const history = await api.getHistory()
  addCheck(checks, 'completed recording is present in History', history.length === 1)

  const report: VerificationReport = { checks, timingsMs, settingsRevisions: revisions }
  startStatusPerf(report)
}

verify().catch((reason: unknown) => {
  const message = reason instanceof Error ? reason.message : String(reason)
  console.error(`NATIVE_RECORDING_PROBE_ERROR ${message}`)
  // The feature-gated command terminates the process and preserves the error in stderr.
  invoke('perf_report_failed', { message }).catch(console.error)
})
