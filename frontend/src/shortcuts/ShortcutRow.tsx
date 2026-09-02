import { useEffect, useRef, useState } from 'react'

import { messageFrom } from '../app/formatting'
import { presentShortcut } from '../shortcut'
import { getAppStatus, getShortcutStatus, stopRecording } from '../tauri'
import type { AppStatus } from '../generated/ipc'
import { useShortcutVerification } from './useShortcutVerification'

export function ShortcutRow({
  status,
  repairing,
  onRepair,
  onRetry,
  onError,
}: {
  status: AppStatus
  repairing: boolean
  onRepair: () => void
  onRetry: () => Promise<void>
  onError: (message: string) => void
}) {
  const shortcut = presentShortcut(status.shortcut)
  const currentIdentity = shortcut.verificationIdentity
  const [rawVerification, setRawVerification] = useState(() => ({
    rawAt: localStorage.getItem('echo-shortcut-verified-at'),
    storedIdentity: localStorage.getItem('echo-shortcut-verified-identity'),
  }))
  const verification = useShortcutVerification(
    rawVerification.rawAt,
    rawVerification.storedIdentity,
    currentIdentity,
  )
  const [phase, setPhase] = useState<'idle' | 'arming' | 'listening' | 'timed-out'>('idle')
  const [retrying, setRetrying] = useState(false)
  const attempt = useRef(0)
  const verificationActive = useRef(false)
  const verificationContext = useRef<ShortcutTestContext | null>(null)
  const pollInFlightAttempt = useRef<number | null>(null)
  const pollTimer = useRef<number | null>(null)
  const timeoutTimer = useRef<number | null>(null)
  const completeVerification = (identity: string) => {
    verificationActive.current = false
    verificationContext.current = null
    attempt.current += 1
    if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
    if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    const now = Math.floor(Date.now() / 1000)
    const rawAt = String(now)
    localStorage.setItem('echo-shortcut-verified-at', rawAt)
    localStorage.setItem('echo-shortcut-verified-identity', identity)
    setRawVerification({ rawAt, storedIdentity: identity })
    setPhase('idle')
  }
  const failVerificationAttempt = (attemptId: number, reason: unknown) => {
    if (attempt.current !== attemptId) return
    attempt.current += 1
    verificationActive.current = false
    verificationContext.current = null
    if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
    if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    pollTimer.current = null
    timeoutTimer.current = null
    setPhase('timed-out')
    onError(messageFrom(reason))
  }

  useEffect(() => {
    return () => {
      const attemptId = attempt.current
      attempt.current += 1
      if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
      if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
      const context = verificationContext.current
      if (
        verificationActive.current
        && context != null
        && pollInFlightAttempt.current !== attemptId
      ) {
        stopAttributedShortcutRecording(context).catch(() => undefined)
      }
      verificationActive.current = false
      verificationContext.current = null
    }
  }, [])

  const stopVerifiedRecording = async (activation: string, attemptId: number) => {
    try {
      await stopRecording(activation)
      for (let check = 0; check < 20; check += 1) {
        if (!(await getAppStatus()).recording) return true
        await new Promise((resolve) => window.setTimeout(resolve, 25))
      }
      if (attempt.current === attemptId) {
        onError('Echo could not confirm that the shortcut recording stopped.')
      }
    } catch (reason) {
      if (attempt.current === attemptId) onError(messageFrom(reason))
    }
    return false
  }

  const start = async () => {
    const attemptId = attempt.current + 1
    attempt.current = attemptId
    if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
    if (timeoutTimer.current != null) window.clearTimeout(timeoutTimer.current)
    setPhase('arming')
    verificationActive.current = true
    verificationContext.current = null

    try {
      const baseline = presentShortcut(await getShortcutStatus())
      if (attempt.current !== attemptId) return
      const expectedActivationSource = baseline.expectedActivationSource
      const baselineIdentity = baseline.verificationIdentity
      if (expectedActivationSource == null || baselineIdentity == null) {
        verificationActive.current = false
        setPhase('timed-out')
        return
      }
      const context = {
        baselineActivation: baseline.activation,
        expectedActivationSource,
        verificationIdentity: baselineIdentity,
      }
      verificationContext.current = context
      setPhase('listening')

      const poll = async () => {
        pollInFlightAttempt.current = attemptId
        try {
          const next = presentShortcut(await getShortcutStatus())
          const activation = attributedShortcutActivation(next, context)
          if (attempt.current !== attemptId) {
            const cleanup = activation == null
              ? stopAttributedShortcutRecording(context)
              : stopAttributedShortcutRecording(context, activation)
            void cleanup.catch(() => undefined)
            return
          }
          if (activation != null) {
            const stopped = await stopVerifiedRecording(activation, attemptId)
            if (attempt.current !== attemptId) return
            if (!stopped) {
              verificationActive.current = false
              verificationContext.current = null
              setPhase('timed-out')
              return
            }
            completeVerification(baselineIdentity)
            return
          }
          pollTimer.current = window.setTimeout(() => {
            poll().catch((reason: unknown) => failVerificationAttempt(attemptId, reason))
          }, 100)
        } finally {
          if (pollInFlightAttempt.current === attemptId) {
            pollInFlightAttempt.current = null
          }
        }
      }
      pollTimer.current = window.setTimeout(() => {
        poll().catch((reason: unknown) => failVerificationAttempt(attemptId, reason))
      }, 100)
      timeoutTimer.current = window.setTimeout(() => {
        if (attempt.current !== attemptId) return
        attempt.current += 1
        verificationActive.current = false
        const context = verificationContext.current
        verificationContext.current = null
        if (pollTimer.current != null) window.clearTimeout(pollTimer.current)
        setPhase('timed-out')
        if (context != null && pollInFlightAttempt.current !== attemptId) {
          stopAttributedShortcutRecording(context).catch((reason: unknown) =>
            onError(messageFrom(reason)),
          )
        }
      }, 10_000)
    } catch (reason) {
      failVerificationAttempt(attemptId, reason)
    }
  }

  const repair = () => {
    localStorage.removeItem('echo-shortcut-verified-at')
    localStorage.removeItem('echo-shortcut-verified-identity')
    setRawVerification({ rawAt: null, storedIdentity: null })
    onRepair()
  }
  const retry = async () => {
    setRetrying(true)
    try {
      await onRetry()
    } finally {
      setRetrying(false)
    }
  }
  const setup = shortcut.gnomeSetup

  return (
    <div className="setting-row">
      <div>
        <strong>Toggle shortcut</strong>
        <span>{shortcut.description}</span>
        {shortcut.manualCommand ? (
          <span>
            Bind <kbd>{shortcut.desired}</kbd> to <code>{shortcut.manualCommand}</code> in your compositor settings.
          </span>
        ) : null}
      </div>
      <div className="setting-actions">
        <kbd>{shortcut.display}</kbd>
        {setup?.state === 'missing' || setup?.state === 'stale' ? (
          <button type="button" className="compact-button" disabled={repairing} onClick={repair}>
            {repairing
              ? 'Updating…'
              : setup.state === 'missing'
                ? 'Set up GNOME shortcut'
                : 'Repair GNOME shortcut'}
          </button>
        ) : null}
        {shortcut.canRetry ? (
          <button
            type="button"
            className="compact-button"
            disabled={retrying}
            onClick={() => {
              retry().catch(() => undefined)
            }}
          >
            {retrying ? 'Retrying…' : 'Retry shortcut'}
          </button>
        ) : null}
        <span className="status-note" data-tone={shortcut.tone}>
          <span className="status-dot" data-tone={shortcut.tone} aria-hidden="true" />
          {shortcut.statusLabel}
        </span>
        {phase === 'listening' ? (
          <span className="status-note" data-tone="ok">
            <span className="status-dot" data-tone="ok" aria-hidden="true" />
            Listening… press your shortcut
          </span>
        ) : (
          <button
            type="button"
            className="compact-button"
            disabled={status.recording || !shortcut.testable}
            onClick={() => {
              start().catch(() => undefined)
            }}
          >
            Test shortcut
          </button>
        )}
        {phase === 'timed-out' ? (
          <span className="status-note" data-tone="attention">
            <span className="status-dot" data-tone="attention" aria-hidden="true" />
            No keypress seen — check the binding
          </span>
        ) : null}
        {phase === 'idle' && shortcut.testable && verification?.identity === currentIdentity ? (
          <span className="status-note" data-tone="ok">
            <span className="status-dot" data-tone="ok" aria-hidden="true" />
            Verified {new Date(verification.at * 1000).toLocaleDateString()}
          </span>
        ) : null}
      </div>
    </div>
  )
}

interface ShortcutTestContext {
  baselineActivation: string | null
  expectedActivationSource: string
  verificationIdentity: string
}

function attributedShortcutActivation(
  shortcut: ReturnType<typeof presentShortcut>,
  context: ShortcutTestContext,
) {
  const activation = shortcut.activation
  return activation !== context.baselineActivation &&
    activation?.startsWith(`${context.expectedActivationSource}:`) === true &&
    shortcut.verificationIdentity === context.verificationIdentity
    ? activation
    : null
}

async function stopAttributedShortcutRecording(
  context: ShortcutTestContext,
  observedActivation?: string,
) {
  const activation =
    observedActivation ??
    attributedShortcutActivation(presentShortcut(await getShortcutStatus()), context)
  if (activation == null) return false
  return stopRecording(activation)
}
