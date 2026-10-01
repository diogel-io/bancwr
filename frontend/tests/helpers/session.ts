// Signs a test in as a role. The global middleware re-reads the session on every navigation,
// mounts included, so setting the auth state alone does not last: the session endpoint has to
// agree with it.
import { registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import type { Role } from '#shared/types/bunker'
import type { AuthState } from '~/composables/useAuth'
import { useState } from '#imports'

let current: Role | undefined

registerEndpoint('/api/auth/session', () => {
  if (!current) throw createError({ status: 401, data: { error: 'not_authenticated' } })
  return { pubkey: 'a'.repeat(64), npub: 'npub1a', role: current }
})

export function signInAs(role: Role | undefined) {
  current = role
  useState<AuthState>('auth').value = role
    ? { status: 'signed-in', pubkey: 'a'.repeat(64), npub: 'npub1a', role }
    : { status: 'signed-out' }
}
