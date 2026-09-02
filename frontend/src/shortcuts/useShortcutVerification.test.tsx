import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SHORTCUT_VERIFICATION_MAX_AGE_SECONDS } from './shortcutVerification'
import { useShortcutVerification } from './useShortcutVerification'

const IDENTITY = 'portal:Super+Alt+Space'

interface ShortcutVerificationProps {
  rawAt: string | null
  storedIdentity: string | null
  currentIdentity: string | null
}

describe('useShortcutVerification', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('expires a valid record immediately after the inclusive 30-day boundary', async () => {
    vi.useFakeTimers()
    const at = 2_000_000_000
    vi.setSystemTime((at + SHORTCUT_VERIFICATION_MAX_AGE_SECONDS) * 1000)

    const { result } = renderHook(() => useShortcutVerification(
      String(at),
      IDENTITY,
      IDENTITY,
    ))

    expect(result.current).toEqual({ at, identity: IDENTITY })

    await act(async () => vi.advanceTimersByTimeAsync(999))
    expect(result.current).toEqual({ at, identity: IDENTITY })

    await act(async () => vi.advanceTimersByTimeAsync(1))
    expect(result.current).toBeNull()
  })

  it('accepts a stored record when the current identity resolves to a match', () => {
    vi.useFakeTimers()
    const now = 2_000_000_000
    vi.setSystemTime(now * 1000)
    const initialProps: ShortcutVerificationProps = {
      rawAt: String(now - 1),
      storedIdentity: IDENTITY,
      currentIdentity: null,
    }
    const { result, rerender } = renderHook(
      ({ rawAt, storedIdentity, currentIdentity }: ShortcutVerificationProps) =>
        useShortcutVerification(rawAt, storedIdentity, currentIdentity),
      { initialProps },
    )

    expect(result.current).toBeNull()

    rerender({ ...initialProps, currentIdentity: IDENTITY })

    expect(result.current).toEqual({ at: now - 1, identity: IDENTITY })
  })
})
