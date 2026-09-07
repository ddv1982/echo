export function newestSnapshot<T extends { revision: number }>(current: T | null, next: T): T | null {
  if (current == null) return next
  return next.revision > current.revision ? next : current
}
