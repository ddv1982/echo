import { useEffect, useState } from 'react'

export function useElapsedSeconds(startedAt: number | null) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (startedAt === null) return
    const timer = window.setInterval(() => setNow(Date.now()), 250)
    return () => window.clearInterval(timer)
  }, [startedAt])
  return startedAt === null ? 0 : Math.max(0, Math.floor((now - startedAt) / 1000))
}
