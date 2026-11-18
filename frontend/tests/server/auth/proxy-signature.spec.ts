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
      nonce: '0123456789abcdef0123456789abcdef',
      method: 'get',
      path: '/api/bunker/whoami?x=1',
      identity: 'ab'.repeat(32),
      body: ''
    })).toBe('4a168ca21125d31242716977c40277da3b80864560281de9fc678a0940ea5267')
  })

  it('matches the vector for the service identity', () => {
    expect(proxySignature(SECRET, {
      timestamp: 1_800_000_000,
      nonce: 'fedcba9876543210fedcba9876543210',
      method: 'GET',
      path: `/api/bunker/team/by-pubkey/${'cd'.repeat(32)}`,
      identity: SERVICE_IDENTITY,
      body: ''
    })).toBe('9096f34ab4eedd93bc53d4adfdecff42a7aea3a458f64be56a9af325462248f5')
  })

  it('matches the vector for a request with a body', () => {
    expect(proxySignature(SECRET, {
      timestamp: 1_800_000_000,
      nonce: '00112233445566778899aabbccddeeff',
      method: 'POST',
      path: '/api/bunker/connections/tokens',
      identity: 'ab'.repeat(32),
      body: '{"label":"x","kinds":[1]}'
    })).toBe('9570f30f7540846ef780bebc5642826c2ece117d0fe050723d3968e60fba86fd')
  })

  it('changes with the body', () => {
    const input = { timestamp: 1_800_000_000, nonce: '0'.repeat(32), method: 'POST', path: '/api/bunker/team', identity: 'ab'.repeat(32) }
    expect(proxySignature(SECRET, { ...input, body: '{"role":"signer"}' }))
      .not.toBe(proxySignature(SECRET, { ...input, body: '{"role":"administrator"}' }))
  })

  it('builds the four headers with a current timestamp', () => {
    const before = Math.floor(Date.now() / 1000)
    const headers = proxyHeaders(SECRET, SERVICE_IDENTITY, 'GET', '/api/bunker/status')
    const timestamp = Number(headers[PROXY_HEADERS.timestamp])
    const nonce = headers[PROXY_HEADERS.nonce]!

    expect(headers[PROXY_HEADERS.identity]).toBe('service')
    expect(timestamp).toBeGreaterThanOrEqual(before)
    expect(nonce).toMatch(/^[0-9a-f]{32}$/)
    expect(headers[PROXY_HEADERS.signature]).toBe(proxySignature(SECRET, {
      timestamp, nonce, method: 'GET', path: '/api/bunker/status', identity: SERVICE_IDENTITY, body: ''
    }))
  })

  it('signs the body it is given', () => {
    const body = '{"relays":["wss://a.example"]}'
    const headers = proxyHeaders(SECRET, 'ab'.repeat(32), 'PUT', '/api/bunker/relays', body)
    expect(headers[PROXY_HEADERS.signature]).toBe(proxySignature(SECRET, {
      timestamp: Number(headers[PROXY_HEADERS.timestamp]),
      nonce: headers[PROXY_HEADERS.nonce]!,
      method: 'PUT',
      path: '/api/bunker/relays',
      identity: 'ab'.repeat(32),
      body
    }))
  })

  it('uses a fresh nonce for every request, so identical requests are not replays', () => {
    const nonces = new Set(Array.from({ length: 20 }, () => proxyHeaders(SECRET, SERVICE_IDENTITY, 'GET', '/api/bunker/status')[PROXY_HEADERS.nonce]))
    expect(nonces.size).toBe(20)
  })
})
