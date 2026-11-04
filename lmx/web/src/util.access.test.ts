import { describe, expect, it } from 'vitest'
import { accessBadge, formatIsoDate, timeLeft } from './util'

const NOW = Date.parse('2026-11-04T00:00:00Z')

describe('access helpers', () => {
  it('labels restrictions', () => {
    expect(accessBadge(null)).toBe('')
    expect(accessBadge({ kind: 'paid', ends_at: null }, NOW)).toBe('要Buzz')
    expect(accessBadge({ kind: 'early', ends_at: '2026-11-08T23:28:12.560Z' }, NOW)).toBe('先行公開 あと4日')
    expect(accessBadge({ kind: 'early', ends_at: '2026-11-04T05:30:00Z' }, NOW)).toBe('先行公開 あと5時間')
    expect(accessBadge({ kind: 'early', ends_at: '2026-11-03T00:00:00Z' }, NOW)).toBe('先行公開')
  })
  it('time left and dates', () => {
    expect(timeLeft('2026-11-04T00:20:00Z', NOW)).toBe('あと20分')
    expect(timeLeft('2026-11-01T00:00:00Z', NOW)).toBe('')
    expect(formatIsoDate('bad')).toBe('')
    expect(formatIsoDate('2026-11-03T12:00:00.000Z')).toMatch(/^2026\/11\/0[34]$/)
  })
})
