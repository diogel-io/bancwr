import { describe, it, expect } from 'vitest'
import type { Role } from '#shared/types/bunker'
import { canOpen, isForbidden, pageTitle } from '~/utils/access'

// The whole matrix from #26, so a change to who may open what is a deliberate edit here.
const MATRIX: [string, Record<Role, boolean>][] = [
  ['/', { administrator: true, user: true, signer: true }],
  ['/config', { administrator: true, user: false, signer: false }],
  ['/team', { administrator: true, user: false, signer: false }],
  ['/logs', { administrator: true, user: false, signer: false }],
  ['/profile', { administrator: true, user: true, signer: true }],
  ['/connections', { administrator: true, user: true, signer: false }],
  ['/follows', { administrator: true, user: true, signer: false }],
  ['/relays', { administrator: true, user: true, signer: false }]
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
    expect(canOpen('user', '/logs#top')).toBe(false)
  })

  it('leaves unknown paths open, so Nuxt\'s 404 still applies', () => {
    expect(canOpen('signer', '/nowhere')).toBe(true)
  })

  it('names pages for the permission-denied message', () => {
    expect(pageTitle('/config')).toBe('Config')
    expect(pageTitle('/nowhere')).toBe('/nowhere')
  })

  it('recognises the bunker\'s 403, whichever field carries it', () => {
    expect(isForbidden({ status: 403 })).toBe(true)
    expect(isForbidden({ statusCode: 403 })).toBe(true)
    expect(isForbidden({ statusCode: 500 })).toBe(false)
    expect(isForbidden(null)).toBe(false)
  })
})
