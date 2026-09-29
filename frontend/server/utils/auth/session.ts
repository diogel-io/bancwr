// SPIKE (#23). The session is h3's own sealed cookie (useSession), the same primitive
// nuxt-auth-utils wraps, without the module's OAuth and password-hashing dependencies.
import type { H3Event } from 'h3'
import { createError, useSession } from 'h3'
import { SESSION_MAX_AGE_SECONDS } from './constants'

export interface BancwrSessionData {
  /** The signed-in pubkey (hex). Nothing else: no role, which is re-read per request. */
  pubkey?: string
  authenticatedAt?: number
}

export function useBancwrSession(event: H3Event, password: string) {
  if (!password || password.length < 32) {
    // Fail closed: without a sealing key there must be no session at all.
    throw createError({ statusCode: 500, statusMessage: 'NUXT_SESSION_PASSWORD must be set (32+ characters)' })
  }
  return useSession<BancwrSessionData>(event, {
    name: 'bancwr-session',
    password,
    maxAge: SESSION_MAX_AGE_SECONDS,
    cookie: { httpOnly: true, secure: true, sameSite: 'strict', path: '/' }
  })
}
