import { Check } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { SectionHeading } from '../app/chrome'
import { messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { MicrophoneChooser } from '../settings/MicrophoneChooser'
import { useMicrophoneController } from '../microphones/useMicrophoneController'
import { SpeechSetupSection } from '../settings/SpeechSetupSection'
import { applySetupProgress, classifySetupEvent } from '../setup'
import { presentShortcut } from '../shortcut'
import { useShortcutVerification } from '../shortcuts/useShortcutVerification'
import {
  getReadiness,
  onSetupEvent,
} from '../tauri'
import type {
  AppStatus,
  Readiness,
  SetupEvent,
} from '../generated/ipc'

export function SetupChecklist({
  status,
  onOpenSettings,
}: {
  status: AppStatus
  onOpenSettings: () => void
}) {
  const [readiness, setReadiness] = useState<Readiness | null>(null)
  const [setupError, setSetupError] = useState<string | null>(null)
  const mountedRef = useRef(true)
  const readinessVersion = useRef(0)
  const reportSetupError = useCallback((reason: unknown) => {
    if (mountedRef.current) setSetupError(messageFrom(reason))
  }, [])
  const { microphones, micTest, testingMic, refresh: refreshMicrophones, selectMicrophone, testMicrophone } = useMicrophoneController(reportSetupError)
  const applyFetchedReadiness = useCallback((next: Readiness, version: number) => {
    if (mountedRef.current && readinessVersion.current === version) setReadiness(next)
  }, [])

  useEffect(() => {
    let current = true
    mountedRef.current = true
    const version = ++readinessVersion.current
    void getReadiness().then((next) => {
      if (current) applyFetchedReadiness(next, version)
    }).catch((reason: unknown) => {
      if (current && mountedRef.current) reportSetupError(reason)
    })
    void refreshMicrophones().catch((reason: unknown) => {
      if (current) reportSetupError(reason)
    })
    return () => {
      current = false
      mountedRef.current = false
      readinessVersion.current += 1
    }
  }, [applyFetchedReadiness, refreshMicrophones, reportSetupError])

  const handleSetupEvent = useCallback((event: SetupEvent) => {
    if (!mountedRef.current) return
    const classified = classifySetupEvent(event)
    if (classified.kind === 'incremental') {
      readinessVersion.current += 1
      setReadiness((current) => current && applySetupProgress(current, classified.event))
    }
    if (classified.kind === 'terminal' && classified.error != null) {
      setSetupError(classified.error)
    }
  }, [])
  const getSetupRefresh = useCallback((event: SetupEvent) => {
    if (classifySetupEvent(event).kind === 'incremental') return null
    return () => {
      if (!mountedRef.current) return Promise.resolve(() => undefined)
      const version = ++readinessVersion.current
      return getReadiness().then((next) => () => {
        applyFetchedReadiness(next, version)
      })
    }
  }, [applyFetchedReadiness])
  useAsyncSubscription({
    subscribe: onSetupEvent,
    onEvent: handleSetupEvent,
    getRefresh: getSetupRefresh,
    onError: reportSetupError,
  })

  const identity = presentShortcut(status.shortcut).verificationIdentity
  const verification = useShortcutVerification(
    localStorage.getItem('echo-shortcut-verified-at'),
    localStorage.getItem('echo-shortcut-verified-identity'),
    identity,
  )
  const verified = verification != null
  const items = [
    { key: 'mic', done: readiness?.microphoneReady ?? status.microphoneReady, label: 'Microphone ready' },
    { key: 'engine', done: readiness?.speechReady ?? status.engineReady, label: 'Speech engine and model installed' },
    { key: 'dictation', done: readiness?.hasSuccessfulDictation ?? false, label: 'First dictation complete' },
    { key: 'shortcut', done: verified, label: verified ? 'Shortcut verified' : 'Shortcut bound' },
  ]
  if (readiness?.firstRunComplete && verified) return null
  return (
    <section className="panel checklist" aria-label="Finish setup">
      <SectionHeading title="Finish setup" subtitle="The first-run job is one successful dictation." />
      {setupError ? <div role="alert" className="error-banner">{setupError}</div> : null}
      {readiness && !readiness.microphoneReady && microphones ? (
        <div className="first-run-step">
          <strong>1 · Choose and test a microphone</strong>
          <MicrophoneChooser
            snapshot={microphones}
            test={micTest}
            testing={testingMic}
            onRefresh={() => {
              if (!mountedRef.current) return
              const version = ++readinessVersion.current
              void Promise.all([refreshMicrophones(), getReadiness()])
                .then(([, next]) => applyFetchedReadiness(next, version))
                .catch(reportSetupError)
            }}
            onSelect={(id) => {
              const version = ++readinessVersion.current
              selectMicrophone(id, () => getReadiness().then((next) => applyFetchedReadiness(next, version)))
            }}
            onTest={(id, fallback) => {
              const version = ++readinessVersion.current
              testMicrophone(id, fallback, async (failed) => {
                const [, next] = await Promise.all([
                  failed ? refreshMicrophones() : Promise.resolve(),
                  getReadiness(),
                ])
                applyFetchedReadiness(next, version)
              })
            }}
          />
        </div>
      ) : null}
      {readiness && !readiness.speechReady ? (
        <div className="first-run-step">
          <strong>2 · Set up local speech</strong>
          <SpeechSetupSection
            readiness={readiness}
            guided
            onRefresh={() => {
              if (!mountedRef.current) return
              const version = ++readinessVersion.current
              void getReadiness().then((next) => {
                applyFetchedReadiness(next, version)
              }).catch(reportSetupError)
            }}
            onError={reportSetupError}
          />
        </div>
      ) : null}
      {items.map((item) => (
        <div className="checklist-item" data-done={item.done} key={item.key}>
          <span className="checklist-check" aria-hidden="true">
            {item.done ? <Check size={13} /> : null}
          </span>
          <span className="checklist-label">{item.label}</span>
          {!item.done && item.key === 'shortcut' ? (
            <button type="button" className="compact-button" onClick={onOpenSettings}>
              Open Settings
            </button>
          ) : null}
        </div>
      ))}
    </section>
  )
}
