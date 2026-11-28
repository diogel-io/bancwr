import { describe, it, expect } from 'vitest'
import type { Role } from '#shared/types/bunker'
import { canOpen, isForbidden, isLeadRoute, pageTitle } from '~/utils/access'

// The whole matrix from #77 (which replaced #26's) and #143, so a change to who may open what is a
// deliberate edit here. Admin is a superset of the signer.
const MATRIX: [string, Record<Role, boolean>][] = [
  ['/', { administrator: true, signer: true}],
  ['/config', { administrator: true, signer: false}],
  ['/logs', { administrator: true, signer: false}],
  ['/team', { administrator: true, signer: false}],
  [`/team/${'b'.repeat(64)}`, { administrator: true, signer: false}],
  ['/profile', { administrator: true, signer: true}],
  ['/follows', { administrator: true, signer: true}],
  ['/relays', { administrator: true, signer: true}],
  ['/connections', { administrator: true, signer: true}],
  // An identity's own account (#150): administrators only.
  [`/identities/${'b'.repeat(64)}/profile`, { administrator: true, signer: false}],
  [`/identities/${'b'.repeat(64)}/follows`, { administrator: true, signer: false}],
  [`/identities/${'b'.repeat(64)}/relays`, { administrator: true, signer: false}],
  // Departments (#147): administrators; a lead too, below.
  ['/departments', { administrator: true, signer: false}]
]

describe('access matrix', () => {
  for (const [path, roles] of MATRIX) {
    for (const [role, allowed] of Object.entries(roles) as [Role, boolean][]) {
      it(`${role} ${allowed ? 'may' : 'may not'} open ${path}`, () => {
        expect(canOpen(role, path)).toBe(allowed)
      })
    }
  }

  it('lets a department lead open Departments and their identities\' pages, and nothing else more (#147)', () => {
    const identity = `/identities/${'b'.repeat(64)}`
    for (const path of ['/departments', `${identity}/profile`, `${identity}/follows`, `${identity}/relays`]) {
      expect(canOpen('signer', path, true), path).toBe(true)
      expect(isLeadRoute(path), path).toBe(true)
    }
    for (const path of ['/config', '/logs', '/team', `/team/${'b'.repeat(64)}`]) {
      expect(canOpen('signer', path, true), path).toBe(false)
      expect(isLeadRoute(path), path).toBe(false)
    }
  })

  it('ignores a trailing slash, query or fragment', () => {
    expect(canOpen('signer', '/config/')).toBe(false)
    expect(canOpen('signer', '/config?tab=1')).toBe(false)
    expect(canOpen('signer', '/logs#top')).toBe(false)
    expect(canOpen('signer', `/team/${'b'.repeat(64)}/`)).toBe(false)
  })

  it('matches a member\'s profile by one path segment only', () => {
    expect(canOpen('administrator', '/team/npub1abc')).toBe(true)
    expect(canOpen('signer', '/team/npub1abc')).toBe(false)
    // Deeper paths are not a member's profile: left to Nuxt's 404.
    expect(canOpen('signer', '/team/npub1abc/more')).toBe(true)
  })

  it('keeps the team and members\' profiles for administrators alone (#143)', () => {
    expect(canOpen('signer', '/team')).toBe(false)
    expect(canOpen('signer', '/team/npub1abc')).toBe(false)
    expect(canOpen('administrator', '/team')).toBe(true)
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
