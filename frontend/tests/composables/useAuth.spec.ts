import { describe, it, expect } from 'vitest'
import { registerEndpoint } from '@nuxt/test-utils/runtime'
import { setResponseStatus } from 'h3'
import { useAuth } from '~/composables/useAuth'

// What the session endpoint answers for an unregistered key, set per test.
let answer: Record<string, unknown> = {}
// As server/api/auth/session.get.ts answers: the status set, and the body as given.
registerEndpoint('/api/auth/session', (event) => {
  setResponseStatus(event, 403)
  return answer
})

describe('useAuth: an unregistered key (#74)', () => {
  it('carries noAdministrator when the bunker has none', async () => {
    answer = { error: 'not_registered', npub: 'npub1x', noAdministrator: true }
    expect(await useAuth().refresh()).toEqual({ status: 'not-registered', npub: 'npub1x', noAdministrator: true })
  })

  it('leaves it off otherwise', async () => {
    answer = { error: 'not_registered', npub: 'npub1x' }
    expect(await useAuth().refresh()).toEqual({ status: 'not-registered', npub: 'npub1x' })
  })
})
