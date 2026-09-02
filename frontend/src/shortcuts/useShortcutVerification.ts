import { useEffect, useState } from 'react'

import {
  parseShortcutVerification,
  SHORTCUT_VERIFICATION_MAX_AGE_SECONDS,
} from './shortcutVerification'

const MAX_TIMEOUT_MILLISECONDS = 2_147_483_647

export function useShortcutVerification(
  rawAt: string | null,
  storedIdentity: string | null,
  currentIdentity: string | null,
) {
  const [nowSeconds, setNowSeconds] = useState(() => Math.floor(Date.now() / 1000))
  const [clockInputs, setClockInputs] = useState(() => ({
    rawAt,
    storedIdentity,
    currentIdentity,
  }))
  const inputsChanged = clockInputs.rawAt !== rawAt
    || clockInputs.storedIdentity !== storedIdentity
    || clockInputs.currentIdentity !== currentIdentity
  const verification = parseShortcutVerification(
    rawAt,
    storedIdentity,
    currentIdentity,
    nowSeconds,
  )
  const verifiedAt = verification?.at ?? null

  useEffect(() => {
    const expiresAt = verifiedAt == null
      ? null
      : verifiedAt + SHORTCUT_VERIFICATION_MAX_AGE_SECONDS + 1
    let delay = 0
    if (!inputsChanged) {
      if (expiresAt == null) return
      delay = Math.min(
        Math.max(0, expiresAt - nowSeconds) * 1000,
        MAX_TIMEOUT_MILLISECONDS,
      )
    }
    const timer = window.setTimeout(() => {
      setClockInputs({ rawAt, storedIdentity, currentIdentity })
      setNowSeconds(Math.floor(Date.now() / 1000))
    }, delay)
    return () => window.clearTimeout(timer)
  }, [currentIdentity, inputsChanged, nowSeconds, rawAt, storedIdentity, verifiedAt])

  return verification
}
