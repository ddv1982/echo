export const SHORTCUT_VERIFICATION_MAX_AGE_SECONDS = 30 * 24 * 60 * 60

export function parseShortcutVerification(
  rawAt: string | null,
  storedIdentity: string | null,
  currentIdentity: string | null,
  nowSeconds: number,
): { at: number; identity: string } | null {
  if (
    rawAt == null
    || storedIdentity == null
    || currentIdentity == null
    || storedIdentity !== currentIdentity
  ) return null

  const at = Number(rawAt)
  if (
    !Number.isFinite(at)
    || !Number.isInteger(at)
    || at <= 0
    || at > nowSeconds
    || nowSeconds - at > SHORTCUT_VERIFICATION_MAX_AGE_SECONDS
  ) return null

  return { at, identity: storedIdentity }
}
