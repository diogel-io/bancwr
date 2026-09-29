// @vitest-environment node
// SPIKE (#23). The same vector is asserted by backend/tests/unit/proxy_auth_test.rs, so the two
// implementations cannot drift apart.
import { describe, it, expect } from 'vitest'
import { proxySignature } from '../../../server/utils/auth/proxy-signature'

describe('proxySignature', () => {
  it('matches the shared test vector', () => {
    expect(proxySignature('bancwr-test-secret', {
      timestamp: 1_800_000_000,
      method: 'get',
      path: '/api/bunker/whoami?x=1',
      pubkey: 'ab'.repeat(32)
    })).toBe('72014519598768d6aa027face9fa3f2f1377a6e6f756ab53b862077311a3b4fc')
  })
})
