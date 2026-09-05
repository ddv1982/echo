export function newestSnapshot<T extends { revision: number }>(current: T | null, next: T): T | null {
  return next.revision >= (current?.revision ?? 0) ? next : current
}
