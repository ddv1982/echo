import { describe, expect, it } from 'vitest'

import { newestSnapshot } from './snapshotFreshness'

type TestSnapshot = { revision: number; value: string }

describe('newestSnapshot', () => {
  it('ignores lower and missing revisions and accepts newer snapshots', () => {
    const current: TestSnapshot = { revision: 4, value: 'current' }
    const missingRevision: TestSnapshot = { revision: 3, value: 'missing' }
    Object.defineProperty(missingRevision, 'revision', { value: undefined })

    expect(newestSnapshot(current, { revision: 3, value: 'old' })).toBe(current)
    expect(newestSnapshot(current, missingRevision)).toBe(current)
    expect(newestSnapshot(null, { revision: 0, value: 'first' })).toEqual({
      revision: 0,
      value: 'first',
    })
    expect(newestSnapshot(current, { revision: 4, value: 'same' })).toBe(current)
    expect(newestSnapshot(current, { revision: 5, value: 'new' })).toEqual({
      revision: 5,
      value: 'new',
    })
  })
})
