// @vitest-environment node
import { describe, it, expect } from 'vitest'
import { authConfigProblems } from '../../../server/utils/auth/config'

const good = { sessionPassword: 's'.repeat(32), proxySecret: 'p'.repeat(32), siteOrigin: 'https://bancwr.example' }

describe('authConfigProblems', () => {
  it('accepts a complete https configuration', () => {
    expect(authConfigProblems(good)).toEqual([])
  })

  it('allows plain http only for localhost', () => {
    for (const siteOrigin of ['http://localhost:3100', 'http://127.0.0.1:3001', 'http://[::1]:3001']) {
      expect(authConfigProblems({ ...good, siteOrigin }), siteOrigin).toEqual([])
    }
    const [problem] = authConfigProblems({ ...good, siteOrigin: 'http://192.168.1.20:3001' })
    expect(problem).toContain('https://')
  })

  it('requires an origin, not a URL with a path or trailing slash', () => {
    for (const siteOrigin of ['', 'bancwr.example', 'https://bancwr.example/', 'https://bancwr.example/admin']) {
      expect(authConfigProblems({ ...good, siteOrigin }), siteOrigin).toHaveLength(1)
    }
  })

  it('requires both secrets at 32 characters or more', () => {
    expect(authConfigProblems({ ...good, sessionPassword: 'short' })[0]).toContain('NUXT_SESSION_PASSWORD')
    expect(authConfigProblems({ ...good, proxySecret: '' })[0]).toContain('NUXT_PROXY_SECRET')
    expect(authConfigProblems({})).toHaveLength(3)
  })
})
