// @vitest-environment node
// The same vectors are asserted by backend/tests/unit/proxy_auth_test.rs, so the two
// implementations cannot drift apart (#25).
import { describe, it, expect } from 'vitest'
import { proxyHeaders, proxySignature, PROXY_HEADERS, SERVICE_IDENTITY } from '../../../server/utils/auth/proxy-signature'

const SECRET = 'bancwr-test-secret'

describe('proxySignature', () => {
  it('matches the vector for a user identity', () => {
    expect(proxySignature(SECRET, {
      timestamp: 1_800_000_000,
      method: 'get',
      path: '/api/bunker/whoami?x=1',
      identity: 'ab'.repeat(32)
    })).toBe('72014519598768d6aa027face9fa3f2f1377a6e6f756ab53b862077311a3b4fc')
  })

  it('matches the vector for the service identity', () => {
    expect(proxySignature(SECRET, {
      timestamp: 1_800_000_000,
      method: 'GET',
      path: `/api/bunker/team/by-pubkey/${'cd'.repeat(32)}`,
      identity: SERVICE_IDENTITY
    })).toBe('5eaf28079c0017b4f359cf886f5c045b9bcf7986fac3c8e92cd7cc2468ddb9af')
  })

  it('builds the three headers with a current timestamp', () => {
    const before = Math.floor(Date.now() / 1000)
    const headers = proxyHeaders(SECRET, SERVICE_IDENTITY, 'GET', '/api/bunker/status')
    const timestamp = Number(headers[PROXY_HEADERS.timestamp])

    expect(headers[PROXY_HEADERS.identity]).toBe('service')
    expect(timestamp).toBeGreaterThanOrEqual(before)
    expect(headers[PROXY_HEADERS.signature]).toBe(proxySignature(SECRET, {
      timestamp, method: 'GET', path: '/api/bunker/status', identity: SERVICE_IDENTITY
    }))
  })
})
