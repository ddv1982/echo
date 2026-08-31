import { summarizeSamples } from './statusPerf'

describe('summarizeSamples', () => {
  it('reports interpolated percentiles without changing the input order', () => {
    const samples = [4, 1, 3, 2]

    const summary = summarizeSamples(samples)
    expect(summary).toMatchObject({
      count: 4,
      minMs: 1,
      p50Ms: 2.5,
      maxMs: 4,
    })
    expect(summary.p95Ms).toBeCloseTo(3.85)
    expect(samples).toEqual([4, 1, 3, 2])
  })

  it('rejects an empty sample set', () => {
    expect(() => summarizeSamples([])).toThrow('sample set is empty')
  })
})
