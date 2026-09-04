import { Check } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { SectionHeading } from '../app/chrome'
import { messageFrom } from '../app/formatting'
import { useAsyncSubscription } from '../hooks/useAsyncSubscription'
import { MicrophoneChooser } from '../settings/MicrophoneChooser'
import { SpeechSetupSection } from '../settings/SpeechSetupSection'
import { applySetupProgress, classifySetupEvent } from '../setup'
import { presentShortcut } from '../shortcut'
import { useShortcutVerification } from '../shortcuts/useShortcutVerification'
import {
  getMicrophones,
  getReadiness,
  onSetupEvent,
  setMicrophone,
  testInputDevice,
  testMicrophoneFallback,
} from '../tauri'
import type {
  AppStatus,
  MicrophoneSnapshot,
  MicrophoneTestResult,
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
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const [setupError, setSetupError] = useState<string | null>(null)
  const mountedRef = useRef(true)
  const micTestVersion = useRef(0)
  const reportSetupError = useCallback((reason: unknown) => {
    if (mountedRef.current) setSetupError(messageFrom(reason))
  }, [])

  useEffect(() => {
    let current = true
    mountedRef.current = true
    void getReadiness().then((next) => {
      if (current && mountedRef.current) setReadiness(next)
    }).catch((reason: unknown) => {
      if (current && mountedRef.current) reportSetupError(reason)
    })
    void getMicrophones().then((next) => {
      if (current && mountedRef.current) setMicrophones(next)
    }).catch((reason: unknown) => {
      if (current && mountedRef.current) reportSetupError(reason)
    })
    return () => {
      current = false
      mountedRef.current = false
      micTestVersion.current += 1
    }
  }, [reportSetupError])

  const handleSetupEvent = useCallback((event: SetupEvent) => {
    if (!mountedRef.current) return
    const classified = classifySetupEvent(event)
    if (classified.kind === 'incremental') {
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
      return getReadiness().then((next) => () => {
        if (mountedRef.current) setReadiness(next)
      })
    }
  }, [])
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
      <SectionHeading title="Finish setup" subtitle="A few checks before your first dictation." />
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
              void Promise.all([getMicrophones(), getReadiness()])
                .then(([nextMicrophones, nextReadiness]) => {
                  if (!mountedRef.current) return
                  setMicrophones(nextMicrophones)
                  setReadiness(nextReadiness)
                })
                .catch(reportSetupError)
            }}
            onSelect={(id) => {
              if (!mountedRef.current) return
              micTestVersion.current += 1
              setMicTest(null)
              void setMicrophone(id)
                .then((nextMicrophones) => {
                  if (!mountedRef.current) return null
                  setMicrophones(nextMicrophones)
                  return getReadiness()
                })
                .then((next) => {
                  if (next && mountedRef.current) setReadiness(next)
                })
                .catch(reportSetupError)
            }}
            onTest={(id, fallback) => {
              if (!mountedRef.current) return
              const version = ++micTestVersion.current
              setTestingMic(true)
              const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
              void run
                .then((result) => {
                  if (!mountedRef.current || micTestVersion.current !== version) return null
                  setMicTest(result)
                  return getReadiness()
                })
                .then((next) => {
                  if (next && mountedRef.current && micTestVersion.current === version) setReadiness(next)
                })
                .catch((reason: unknown) => {
                  if (mountedRef.current && micTestVersion.current === version) reportSetupError(reason)
                })
                .finally(() => {
                  if (mountedRef.current && micTestVersion.current === version) setTestingMic(false)
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
              void getReadiness().then((next) => {
                if (mountedRef.current) setReadiness(next)
              }).catch(reportSetupError)
            }}
            onError={reportSetupError}
          />
        </div>
      ) : null}
      <div className="checklist-progress">
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
      </div>
    </section>
  )
}
