import { describe, it, expect } from 'vitest'
import { ENCRYPTION_METHODS, LIFETIMES, connectionEnd, isDecrypt, kindLabel, lifetimeLabel, methodLabel, parseKinds, relativeTime, revokedReason } from '~/utils/connections'

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
    expect(revokedReason('restored')).toBe('The bunker was restored from a backup')
    expect(revokedReason(null)).toBe('Ended')
  })

  it('names the encryption methods in the bunker\'s order, and knows which read messages (#97)', () => {
    expect(ENCRYPTION_METHODS.map(m => m.value)).toEqual(['nip44_encrypt', 'nip44_decrypt', 'nip04_encrypt', 'nip04_decrypt'])
    expect(methodLabel('nip44_decrypt')).toBe('Read encrypted messages (NIP-44)')
    expect(methodLabel('nip04_encrypt')).toBe('Encrypt messages (NIP-04, older apps)')
    expect(ENCRYPTION_METHODS.filter(m => isDecrypt(m.value)).map(m => m.value)).toEqual(['nip44_decrypt', 'nip04_decrypt'])
  })

  it('names connection lifetimes and when a connection ends (#98)', () => {
    expect(LIFETIMES.map(l => l.label)).toEqual(['Until revoked', '1 hour', '8 hours', '1 day', '7 days', '30 days', '90 days'])
    expect(lifetimeLabel(24)).toBe('1 day')
    expect(lifetimeLabel(720)).toBe('30 days')
    expect(lifetimeLabel(36)).toBe('36 hours')
    expect(lifetimeLabel(1)).toBe('1 hour')
    expect(lifetimeLabel(8)).toBe('8 hours')
    const now = Date.parse('2026-06-01T12:00:00Z')
    expect(connectionEnd(null, now)).toBe('no time limit')
    expect(connectionEnd('2026-06-03T12:00:00Z', now)).toBe('ends in 2 days')
    expect(connectionEnd('2026-06-01T20:00:00Z', now)).toBe('ends in 8 hours')
    expect(connectionEnd('2026-06-01T12:45:00Z', now)).toBe('ends in 45 minutes')
    expect(connectionEnd('2026-06-01T11:00:00Z', now)).toBe('time limit reached 1 hour ago')
    expect(revokedReason('expired')).toBe('Its time limit ran out')
  })
})
