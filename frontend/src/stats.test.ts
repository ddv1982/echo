import { describe, expect, it } from 'vitest'
import { deriveStats, groupByDay, millisecondsUntilNextLocalDay } from './stats'

const NOW = new Date('2026-08-22T15:00:00') // a Saturday

function requireFixture<T>(value: T | undefined, description: string): T {
  if (value === undefined) throw new Error(`missing test fixture: ${description}`)
  return value
}

function row(text: string, daysAgo: number, hour = 12) {
  const date = new Date(NOW)
  date.setDate(date.getDate() - daysAgo)
  date.setHours(hour, 0, 0, 0)
  return { text, startedAt: Math.floor(date.getTime() / 1000) }
}

describe('deriveStats', () => {
  it('is all zero on empty history', () => {
    expect(deriveStats([], NOW)).toEqual({ words: 0, sessionsThisWeek: 0, dayStreak: 0 })
  })

  it('counts one session', () => {
    const stats = deriveStats([row('hello there', 0)], NOW)
    expect(stats).toEqual({ words: 2, sessionsThisWeek: 1, dayStreak: 1 })
  })

  it('counts a streak across consecutive days', () => {
    const stats = deriveStats(
      [row('one', 0), row('two', 1), row('three', 2), row('gap', 4)],
      NOW,
    )
    expect(stats.dayStreak).toBe(3)
    expect(stats.words).toBe(4)
  })

  it('keeps the streak alive when today has no session yet', () => {
    const stats = deriveStats([row('one', 1), row('two', 2)], NOW)
    expect(stats.dayStreak).toBe(2)
  })

  it('breaks the streak when yesterday is missing too', () => {
    const stats = deriveStats([row('one', 2), row('two', 3)], NOW)
    expect(stats.dayStreak).toBe(0)
  })

  it('counts only this ISO week for sessions', () => {
    // NOW is Saturday; Monday of this week is 2026-08-17.
    const stats = deriveStats([row('this week', 5, 9), row('last week', 6, 9)], NOW)
    expect(stats.sessionsThisWeek).toBe(1)
  })

  it('attributes a cross-midnight session to its capture start for week and streak', () => {
    const monday = new Date('2026-08-17T15:00:00')
    const session = {
      text: 'crossing session',
      startedAt: Math.floor(new Date('2026-08-16T23:59:00').getTime() / 1000),
      completedAt: Math.floor(new Date('2026-08-17T00:01:00').getTime() / 1000),
    }
    const saturday = {
      text: 'prior session',
      startedAt: Math.floor(new Date('2026-08-15T12:00:00').getTime() / 1000),
    }

    const stats = deriveStats([session, saturday], monday)

    expect(stats.sessionsThisWeek).toBe(0)
    expect(stats.dayStreak).toBe(2)
  })
})

describe('groupByDay', () => {
  it('groups Today, Yesterday, then dates, preserving order', () => {
    const items = [row('a', 0), row('b', 0, 9), row('c', 1), row('d', 3)]
    const groups = groupByDay(items, NOW)
    const olderItem = requireFixture(items[3], 'older history item')
    const olderDate = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(
      new Date(olderItem.startedAt * 1000),
    )
    expect(groups.map((group) => group.label)).toEqual(['Today', 'Yesterday', olderDate])
    expect(requireFixture(groups[0], 'Today history group').items.map((item) => item.text))
      .toEqual(['a', 'b'])
    expect(requireFixture(groups[1], 'Yesterday history group').items.map((item) => item.text))
      .toEqual(['c'])
  })

  it('is empty for empty input', () => {
    expect(groupByDay([], NOW)).toEqual([])
  })

  it('groups a cross-midnight session by its capture start', () => {
    const session = {
      text: 'crossing session',
      startedAt: Math.floor(new Date('2026-08-21T23:59:00').getTime() / 1000),
      completedAt: Math.floor(new Date('2026-08-22T00:01:00').getTime() / 1000),
    }

    const groups = groupByDay([session], NOW)

    expect(groups).toHaveLength(1)
    expect(requireFixture(groups[0], 'cross-midnight history group').label).toBe('Yesterday')
  })

  it('uses local calendar days across the spring DST boundary', () => {
    const previousTimezone = process.env.TZ
    process.env.TZ = 'Europe/Amsterdam'
    try {
      const monday = new Date(2026, 2, 30, 12)
      const sunday = new Date(2026, 2, 29, 12)
      const saturday = new Date(2026, 2, 28, 12)
      const rows = [
        { text: 'sunday', startedAt: Math.floor(sunday.getTime() / 1000) },
        { text: 'saturday', startedAt: Math.floor(saturday.getTime() / 1000) },
      ]

      expect(deriveStats(rows, monday).dayStreak).toBe(2)
      expect(requireFixture(groupByDay(rows, monday)[0], 'DST group').label).toBe('Yesterday')
    } finally {
      if (previousTimezone === undefined) delete process.env.TZ
      else process.env.TZ = previousTimezone
    }
  })
})

describe('millisecondsUntilNextLocalDay', () => {
  it('uses local calendar arithmetic across both DST boundaries', () => {
    const previousTimezone = process.env.TZ
    process.env.TZ = 'Europe/Amsterdam'
    try {
      expect(millisecondsUntilNextLocalDay(new Date(2026, 2, 29, 0)))
        .toBe(23 * 60 * 60 * 1000)
      expect(millisecondsUntilNextLocalDay(new Date(2026, 9, 25, 0)))
        .toBe(25 * 60 * 60 * 1000)
    } finally {
      if (previousTimezone === undefined) delete process.env.TZ
      else process.env.TZ = previousTimezone
    }
  })
})
