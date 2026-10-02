import { describe, it, expect } from 'vitest'
import { kindLabel, parseKinds, relativeTime, revokedReason } from '~/utils/connections'

describe('connection display', () => {
  it('names known kinds and shows others as numbers', () => {
    expect(kindLabel(1)).toBe('1 Note')
    expect(kindLabel(0)).toBe('0 Profile')
    expect(kindLabel(31337)).toBe('31337')
  })

  it('parses custom kinds, deduplicated and sorted, and refuses anything else', () => {
    expect(parseKinds('7, 1 1  30023')).toEqual({ kinds: [1, 7, 30023] })
    expect(parseKinds('')).toEqual({ error: 'Choose at least one kind.' })
    expect(parseKinds('1, x')).toHaveProperty('error')
    expect(parseKinds('70000')).toHaveProperty('error')
    expect(parseKinds('-1')).toHaveProperty('error')
  })

  it('says how long ago, or how long until', () => {
    const now = Date.parse('2026-10-02T12:00:00Z')
    expect(relativeTime('2026-10-02T11:59:58Z', now)).toBe('just now')
    expect(relativeTime('2026-10-02T11:57:00Z', now)).toBe('3 minutes ago')
    expect(relativeTime('2026-10-02T11:00:00Z', now)).toBe('1 hour ago')
    expect(relativeTime('2026-09-30T12:00:00Z', now)).toBe('2 days ago')
    expect(relativeTime('2026-10-03T12:00:00Z', now)).toBe('in 1 day')
  })

  it('explains why a connection ended', () => {
    expect(revokedReason('logout')).toBe('The app logged out')
    expect(revokedReason('member_removed')).toBe('Its member was removed from the vault')
    expect(revokedReason(null)).toBe('Ended')
  })
})
