import { describe, it, expect } from 'vitest'
import { resolveBackendUrl, BackendTargetError } from '../../server/utils/backend'

describe('resolveBackendUrl', () => {
  it('passes through a distinct host', () => {
    expect(resolveBackendUrl('http://bunker:3000')).toBe('http://bunker:3000')
  })

  it('normalises away a trailing slash and path', () => {
    expect(resolveBackendUrl('http://bunker:3000/')).toBe('http://bunker:3000')
  })

  it('allows loopback when the port is not our own', () => {
    // Local development: backend on 3000, Nuxt dev server on 3001.
    expect(resolveBackendUrl('http://localhost:3000', { selfPort: 3001 })).toBe('http://localhost:3000')
  })

  it('refuses a loopback target on our own port', () => {
    // The shipped container: PORT=3000, so http://localhost:3000 is this server. This is the
    // case that made #20 recurse rather than fail.
    expect(() => resolveBackendUrl('http://localhost:3000', { selfPort: 3000 }))
      .toThrow(BackendTargetError)
  })

  it.each(['127.0.0.1', '[::1]'])('treats %s as loopback too', (host) => {
    expect(() => resolveBackendUrl(`http://${host}:3000`, { selfPort: '3000' }))
      .toThrow(BackendTargetError)
  })

  it('refuses a target aimed back at the request origin', () => {
    expect(() => resolveBackendUrl('http://bancwr-frontend:3000', {
      requestOrigin: 'http://bancwr-frontend:3000',
    })).toThrow(BackendTargetError)
  })

  it('allows a target that differs from the request origin', () => {
    expect(resolveBackendUrl('http://bunker:3000', { requestOrigin: 'http://localhost:3001' }))
      .toBe('http://bunker:3000')
  })

  it('compares default ports explicitly, so :80 and no port are the same origin', () => {
    expect(() => resolveBackendUrl('http://bunker', { requestOrigin: 'http://bunker:80' }))
      .toThrow(BackendTargetError)
  })

  it('names the variable when the value is not a URL', () => {
    expect(() => resolveBackendUrl('bunker:3000')).toThrow(/NUXT_API_BASE/)
  })

  it('ignores an unparseable request origin rather than failing the request', () => {
    expect(resolveBackendUrl('http://bunker:3000', { requestOrigin: 'not a url' }))
      .toBe('http://bunker:3000')
  })

  it('does nothing when neither origin nor port is known', () => {
    expect(resolveBackendUrl('http://localhost:3000')).toBe('http://localhost:3000')
  })
})
