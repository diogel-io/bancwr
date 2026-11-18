// The session: h3's own sealed cookie (decided in #23; nuxt-auth-utils wraps the same primitive),
// holding nothing but a session id that the server-side registry resolves to a pubkey (#11). The
// registry keeps its entries on disk (#100), so every call to it is awaited: signing out has
// removed the entry before the response goes.
import type { H3Event } from 'h3'
import { useSession } from 'h3'
import { SESSION_MAX_AGE_SECONDS } from './constants'
import { sessionRegistry } from './sessions'

/** The session cookie's name. The origin check (middleware/origin.ts) looks for it too. */
export const SESSION_COOKIE = 'bancwr-session'

interface SessionCookie {
  sid?: string
}

function cookieSession(event: H3Event) {
  return useSession<SessionCookie>(event, {
    name: SESSION_COOKIE,
    password: useRuntimeConfig(event).sessionPassword,
    maxAge: SESSION_MAX_AGE_SECONDS,
    cookie: { httpOnly: true, secure: true, sameSite: 'strict', path: '/' }
  })
}

/** The signed-in pubkey (hex), or undefined. */
export async function sessionPubkey(event: H3Event): Promise<string | undefined> {
  const session = await cookieSession(event)
  return await sessionRegistry.pubkey(session.data.sid)
}

/** Starts a new session for this pubkey, ending any earlier one in this browser. */
export async function startSession(event: H3Event, pubkey: string) {
  const session = await cookieSession(event)
  await sessionRegistry.revoke(session.data.sid)
  await session.clear()
  await session.update({ sid: await sessionRegistry.create(pubkey) })
}

/** Ends the session: the id is revoked, so a copy of the cookie stops working too. */
export async function endSession(event: H3Event) {
  const session = await cookieSession(event)
  await sessionRegistry.revoke(session.data.sid)
  await session.clear()
}
