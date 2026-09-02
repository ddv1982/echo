import { describe, expect, it } from 'vitest'

import {
  parseShortcutVerification,
  SHORTCUT_VERIFICATION_MAX_AGE_SECONDS,
} from './shortcutVerification'

const NOW_SECONDS = 2_000_000_000
const IDENTITY = 'portal:Super+Alt+Space'

describe('shortcut verification records', () => {
  it('uses a 30-day verification lifetime', () => {
    expect(SHORTCUT_VERIFICATION_MAX_AGE_SECONDS).toBe(30 * 24 * 60 * 60)
  })

  it.each([
    ['missing timestamp', null, IDENTITY, IDENTITY],
    ['missing stored identity', String(NOW_SECONDS), null, IDENTITY],
    ['missing current identity', String(NOW_SECONDS), IDENTITY, null],
    ['malformed timestamp', 'not-a-timestamp', IDENTITY, IDENTITY],
    ['NaN timestamp', 'NaN', IDENTITY, IDENTITY],
    ['infinite timestamp', 'Infinity', IDENTITY, IDENTITY],
    ['fractional timestamp', `${NOW_SECONDS - 0.5}`, IDENTITY, IDENTITY],
    ['zero timestamp', '0', IDENTITY, IDENTITY],
    ['negative timestamp', '-1', IDENTITY, IDENTITY],
    ['future timestamp', String(NOW_SECONDS + 1), IDENTITY, IDENTITY],
    [
      'stale timestamp',
      String(NOW_SECONDS - SHORTCUT_VERIFICATION_MAX_AGE_SECONDS - 1),
      IDENTITY,
      IDENTITY,
    ],
    ['identity mismatch', String(NOW_SECONDS), IDENTITY, 'portal:Ctrl+Alt+Space'],
  ] as const)('rejects a %s', (_case, rawAt, storedIdentity, currentIdentity) => {
    expect(parseShortcutVerification(
      rawAt,
      storedIdentity,
      currentIdentity,
      NOW_SECONDS,
    )).toBeNull()
  })

  it('accepts a record exactly at the inclusive age boundary', () => {
    const at = NOW_SECONDS - SHORTCUT_VERIFICATION_MAX_AGE_SECONDS

    expect(parseShortcutVerification(String(at), IDENTITY, IDENTITY, NOW_SECONDS)).toEqual({
      at,
      identity: IDENTITY,
    })
  })

  it('accepts a current record', () => {
    expect(parseShortcutVerification(
      String(NOW_SECONDS),
      IDENTITY,
      IDENTITY,
      NOW_SECONDS,
    )).toEqual({
      at: NOW_SECONDS,
      identity: IDENTITY,
    })
  })
})
