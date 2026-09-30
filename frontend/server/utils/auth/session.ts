// The session: h3's own sealed cookie (decided in #23; nuxt-auth-utils wraps the same primitive),
// holding nothing but a session id that the server-side registry resolves to a pubkey (#11).
import type { H3Event } from 'h3'
import { useSession } from 'h3'
import { SESSION_MAX_AGE_SECONDS } from './constants'
import { sessionRegistry } from './sessions'

interface SessionCookie {
  sid?: string
}

function cookieSession(event: H3Event) {
  return useSession<SessionCookie>(event, {
    name: 'bancwr-session',
    password: useRuntimeConfig(event).sessionPassword,
    maxAge: SESSION_MAX_AGE_SECONDS,
    cookie: { httpOnly: true, secure: true, sameSite: 'strict', path: '/' }
  })
}

/** The signed-in pubkey (hex), or undefined. */
export async function sessionPubkey(event: H3Event): Promise<string | undefined> {
  const session = await cookieSession(event)
  return sessionRegistry.pubkey(session.data.sid)
}

/** Starts a new session for this pubkey, ending any earlier one in this browser. */
export async function startSession(event: H3Event, pubkey: string) {
  const session = await cookieSession(event)
  sessionRegistry.revoke(session.data.sid)
  await session.clear()
  await session.update({ sid: sessionRegistry.create(pubkey) })
}

/** Ends the session: the id is revoked, so a copy of the cookie stops working too. */
export async function endSession(event: H3Event) {
  const session = await cookieSession(event)
  sessionRegistry.revoke(session.data.sid)
  await session.clear()
}
