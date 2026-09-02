export interface UsageStats {
  words: number
  sessionsThisWeek: number
  dayStreak: number
}

export interface StatsRow {
  text: string
  startedAt: number
}

function dayStart(date: Date): number {
  const start = new Date(date)
  start.setHours(0, 0, 0, 0)
  return start.getTime()
}

function weekStart(date: Date): number {
  // ISO week: Monday is day one.
  const start = new Date(dayStart(date))
  const day = (start.getDay() + 6) % 7
  start.setDate(start.getDate() - day)
  return start.getTime()
}

function previousDay(day: number): number {
  const previous = new Date(day)
  previous.setDate(previous.getDate() - 1)
  return dayStart(previous)
}

export function deriveStats(rows: StatsRow[], now: Date): UsageStats {
  const words = rows.reduce((total, row) => {
    const trimmed = row.text.trim()
    return total + (trimmed ? trimmed.split(/\s+/).length : 0)
  }, 0)
  const week = weekStart(now)
  const sessionsThisWeek = rows.filter((row) => row.startedAt * 1000 >= week).length

  const days = new Set(rows.map((row) => dayStart(new Date(row.startedAt * 1000))))
  const today = dayStart(now)
  // A streak counts back from today, or from yesterday when today has no
  // session yet; the day is not lost for being early.
  let cursor = days.has(today) ? today : previousDay(today)
  let dayStreak = 0
  while (days.has(cursor)) {
    dayStreak += 1
    cursor = previousDay(cursor)
  }
  return { words, sessionsThisWeek, dayStreak }
}

export interface DayGroup<T> {
  label: string
  items: T[]
}

export function groupByDay<T extends { startedAt: number }>(items: T[], now: Date): DayGroup<T>[] {
  const today = dayStart(now)
  const yesterday = previousDay(today)
  const groups: DayGroup<T>[] = []
  for (const item of items) {
    const day = dayStart(new Date(item.startedAt * 1000))
    const label =
      day === today
        ? 'Today'
        : day === yesterday
          ? 'Yesterday'
          : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(day)
    const last = groups[groups.length - 1]
    if (last && last.label === label) {
      last.items.push(item)
    } else {
      groups.push({ label, items: [item] })
    }
  }
  return groups
}
