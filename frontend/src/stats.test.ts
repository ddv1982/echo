import { deriveStats, groupByDay } from './stats'

const NOW = new Date('2026-08-22T15:00:00') // a Saturday

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
})

describe('groupByDay', () => {
  it('groups Today, Yesterday, then dates, preserving order', () => {
    const items = [row('a', 0), row('b', 0, 9), row('c', 1), row('d', 3)]
    const groups = groupByDay(items, NOW)
    expect(groups.map((group) => group.label)).toEqual(['Today', 'Yesterday', 'Aug 19, 2026'])
    expect(groups[0].items.map((item) => item.text)).toEqual(['a', 'b'])
    expect(groups[1].items.map((item) => item.text)).toEqual(['c'])
  })

  it('is empty for empty input', () => {
    expect(groupByDay([], NOW)).toEqual([])
  })
})
