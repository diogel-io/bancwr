import { describe, it, expect } from 'vitest'
import type { Role } from '#shared/types/bunker'
import { canOpen, isForbidden, pageTitle } from '~/utils/access'

// The whole matrix from #77 (which replaced #26's), so a change to who may open what is a
// deliberate edit here. Admin is a superset of the other two.
const MATRIX: [string, Record<Role, boolean>][] = [
  ['/', { administrator: true, signer: true, viewer: true }],
  ['/config', { administrator: true, signer: false, viewer: false }],
  ['/logs', { administrator: true, signer: false, viewer: false }],
  ['/team', { administrator: true, signer: false, viewer: true }],
  [`/team/${'b'.repeat(64)}`, { administrator: true, signer: false, viewer: true }],
  ['/profile', { administrator: true, signer: true, viewer: false }],
  ['/follows', { administrator: true, signer: true, viewer: false }],
  ['/relays', { administrator: true, signer: true, viewer: false }],
  ['/connections', { administrator: true, signer: true, viewer: false }]
]

describe('access matrix', () => {
  for (const [path, roles] of MATRIX) {
    for (const [role, allowed] of Object.entries(roles) as [Role, boolean][]) {
      it(`${role} ${allowed ? 'may' : 'may not'} open ${path}`, () => {
        expect(canOpen(role, path)).toBe(allowed)
      })
    }
  }

  it('ignores a trailing slash, query or fragment', () => {
    expect(canOpen('signer', '/config/')).toBe(false)
    expect(canOpen('signer', '/config?tab=1')).toBe(false)
    expect(canOpen('viewer', '/logs#top')).toBe(false)
    expect(canOpen('signer', `/team/${'b'.repeat(64)}/`)).toBe(false)
  })

  it('matches a member\'s profile by one path segment only', () => {
    expect(canOpen('viewer', '/team/npub1abc')).toBe(true)
    expect(canOpen('signer', '/team/npub1abc')).toBe(false)
    // Deeper paths are not a member's profile: left to Nuxt's 404.
    expect(canOpen('signer', '/team/npub1abc/more')).toBe(true)
  })

  it('leaves unknown paths open, so Nuxt\'s 404 still applies', () => {
    expect(canOpen('signer', '/nowhere')).toBe(true)
  })

  it('names pages for the permission-denied message', () => {
    expect(pageTitle('/config')).toBe('Config')
    expect(pageTitle('/team/npub1abc')).toBe('a team member\'s profile')
    expect(pageTitle('/nowhere')).toBe('/nowhere')
  })

  it('recognises the bunker\'s 403, whichever field carries it', () => {
    expect(isForbidden({ status: 403 })).toBe(true)
    expect(isForbidden({ statusCode: 403 })).toBe(true)
    expect(isForbidden({ statusCode: 500 })).toBe(false)
    expect(isForbidden(null)).toBe(false)
  })
})
