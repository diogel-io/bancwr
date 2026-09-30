import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport } from '@nuxt/test-utils/runtime'
import type { AuthState } from '~/composables/useAuth'
import middleware from '~/middleware/auth.global'

const { refresh, navigateTo } = vi.hoisted(() => ({ refresh: vi.fn(), navigateTo: vi.fn((path: string) => `redirect:${path}`) }))
mockNuxtImport('useAuth', () => () => ({ refresh }))
mockNuxtImport('navigateTo', () => navigateTo)

const run = (path: string) => (middleware as (to: { path: string }) => Promise<unknown>)({ path })
const as = (state: AuthState) => refresh.mockResolvedValue(state)

describe('auth middleware', () => {
  beforeEach(() => navigateTo.mockClear())

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
})
