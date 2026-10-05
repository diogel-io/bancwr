import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport } from '@nuxt/test-utils/runtime'
import type { Role } from '#shared/types/bunker'
import type { AuthState } from '~/composables/useAuth'
import middleware from '~/middleware/auth.global'

const { refresh, navigateTo, abortNavigation } = vi.hoisted(() => ({
  refresh: vi.fn(),
  navigateTo: vi.fn((path: string) => `redirect:${path}`),
  abortNavigation: vi.fn((error: unknown) => ({ aborted: error }))
}))
mockNuxtImport('useAuth', () => () => ({ refresh }))
mockNuxtImport('navigateTo', () => navigateTo)
mockNuxtImport('abortNavigation', () => abortNavigation)

const run = (path: string) => (middleware as (to: { path: string }) => Promise<unknown>)({ path })
const as = (state: AuthState) => refresh.mockResolvedValue(state)
const asRole = (role: Role) => as({ status: 'signed-in', pubkey: 'a'.repeat(64), npub: 'npub1a', role })

describe('auth middleware', () => {
  beforeEach(() => {
    navigateTo.mockClear()
    abortNavigation.mockClear()
  })

  it('sends a signed-out visitor to sign-in from any page, on direct entry too', async () => {
    as({ status: 'signed-out' })
    for (const path of ['/', '/config', '/team', '/logs', '/no-access']) {
      expect(await run(path), path).toBe('redirect:/sign-in')
    }
    expect(await run('/sign-in')).toBeUndefined()
  })

  it('keeps an unregistered key on the no-access page', async () => {
    as({ status: 'not-registered', npub: 'npub1x' })
    for (const path of ['/', '/team', '/sign-in']) {
      expect(await run(path), path).toBe('redirect:/no-access')
    }
    expect(await run('/no-access')).toBeUndefined()
  })

  it('lets a signed-in member through, and away from the sign-in pages', async () => {
    as({ status: 'signed-in', pubkey: 'a'.repeat(64), npub: 'npub1a', role: 'administrator' })
    expect(await run('/team')).toBeUndefined()
    expect(await run('/sign-in')).toBe('redirect:/')
    expect(await run('/no-access')).toBe('redirect:/')
  })

  it('lets each role onto the routes it may open, and refuses the rest in place with a 403 (#26, #77)', async () => {
    const member = `/team/${'b'.repeat(64)}`
    const open: Record<Role, string[]> = {
      administrator: ['/', '/config', '/team', member, '/logs', '/profile', '/connections'],
      signer: ['/', '/profile', '/connections'],
      viewer: ['/', '/team', member]
    }
    for (const role of ['administrator', 'signer', 'viewer'] as Role[]) {
      asRole(role)
      for (const path of ['/', '/config', '/team', member, '/logs', '/profile', '/connections']) {
        abortNavigation.mockClear()
        const result = await run(path)
        if (open[role].includes(path)) {
          expect(result, `${role} ${path}`).toBeUndefined()
        } else {
          const error = (result as { aborted: { status: number, fatal: boolean, data: unknown } }).aborted
          expect(error.status, `${role} ${path}`).toBe(403)
          expect(error.fatal).toBe(true)
          expect(error.data).toEqual({ reason: 'forbidden_route', path })
        }
      }
    }
    // Refused in place: never a redirect, so a typed URL cannot loop.
    expect(navigateTo).not.toHaveBeenCalled()
  })
})
