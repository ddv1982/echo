import { useCallback, useEffect, useRef, useState } from 'react'

import type { MicrophoneSnapshot, MicrophoneTestResult } from '../generated/ipc'
import { newestSnapshot } from '../settings/snapshotFreshness'
import { getMicrophones, setMicrophone, testInputDevice, testMicrophoneFallback } from '../tauri'

export function useMicrophoneController(onError: (reason: unknown) => void) {
  const [microphones, setMicrophones] = useState<MicrophoneSnapshot | null>(null)
  const [micTest, setMicTest] = useState<MicrophoneTestResult | null>(null)
  const [testingMic, setTestingMic] = useState(false)
  const active = useRef(true)
  const lifetime = useRef(0)
  const testVersion = useRef(0)
  const testRun = useRef(0)

  useEffect(() => {
    active.current = true
    return () => {
      active.current = false
      lifetime.current += 1
      testVersion.current += 1
    }
  }, [])

  const applySnapshot = useCallback((next: MicrophoneSnapshot) => {
    if (active.current) setMicrophones((current) => newestSnapshot(current, next))
  }, [])

  const refresh = useCallback(async () => {
    if (!active.current) return
    const started = lifetime.current
    const next = await getMicrophones()
    if (active.current && lifetime.current === started) applySnapshot(next)
  }, [applySnapshot])

  const selectMicrophone = useCallback((id: string | null, onSelected: () => Promise<void>) => {
    if (!active.current) return
    const started = lifetime.current
    testVersion.current += 1
    setMicTest(null)
    void setMicrophone(id).then((next) => {
      if (!active.current || lifetime.current !== started) return
      applySnapshot(next)
      return onSelected()
    }).catch((reason: unknown) => {
      if (active.current && lifetime.current === started) onError(reason)
    })
  }, [applySnapshot, onError])

  const testMicrophone = useCallback((
    id: string | null,
    fallback: boolean,
    refreshAfterTest: (failed: boolean) => Promise<void>,
  ) => {
    if (!active.current) return
    const version = ++testVersion.current
    testRun.current = version
    setTestingMic(true)
    const current = () => active.current && testVersion.current === version
    const run = fallback ? testMicrophoneFallback() : testInputDevice(id)
    void run.then(
      (result) => {
        if (!current()) return
        setMicTest(result)
        return refreshAfterTest(result.kind === 'failed')
      },
      (reason: unknown) => {
        if (!current()) return
        onError(reason)
        return refreshAfterTest(true)
      },
    ).catch((reason: unknown) => {
      if (current()) onError(reason)
    }).finally(() => {
      if (active.current && testRun.current === version) setTestingMic(false)
    })
  }, [onError])

  return { microphones, micTest, testingMic, applySnapshot, refresh, selectMicrophone, testMicrophone }
}
