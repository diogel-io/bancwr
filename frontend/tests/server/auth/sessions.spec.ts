// @vitest-environment node
import { describe, it, expect } from 'vitest'
import { SessionRegistry } from '../../../server/utils/auth/sessions'

describe('SessionRegistry', () => {
  it('resolves a live session to its pubkey', () => {
    const registry = new SessionRegistry()
    const id = registry.create('a'.repeat(64))
    expect(id).toMatch(/^[0-9a-f]{64}$/)
    expect(registry.pubkey(id)).toBe('a'.repeat(64))
  })

  it('forgets a revoked session, so a copy of its cookie stops working', () => {
    const registry = new SessionRegistry()
    const id = registry.create('a'.repeat(64))
    registry.revoke(id)
    expect(registry.pubkey(id)).toBeUndefined()
  })

  it('expires a session after its maximum age', () => {
    let now = 1_000
    const registry = new SessionRegistry(60, () => now)
    const id = registry.create('a'.repeat(64))
    now += 60
    expect(registry.pubkey(id)).toBe('a'.repeat(64))
    now += 1
    expect(registry.pubkey(id)).toBeUndefined()
  })

  it('knows nothing of an id it did not issue, or of none', () => {
    const registry = new SessionRegistry()
    expect(registry.pubkey('f'.repeat(64))).toBeUndefined()
    expect(registry.pubkey(undefined)).toBeUndefined()
  })
})
