// Refuses a request that changes something unless it comes from the console's own origin. See
// utils/auth/origin.ts. It runs before every route, the sign-in routes and the proxy to the bunker
// included. The proxy refuses a body that is not JSON itself, on the request it forwards.
import { isSafeMethod, refusal } from '../utils/auth/origin'
import { SESSION_COOKIE } from '../utils/auth/session'

export default defineEventHandler((event) => {
  if (isSafeMethod(event.method)) return

  const refused = refusal({
    method: event.method,
    origin: getHeader(event, 'origin'),
    secFetchSite: getHeader(event, 'sec-fetch-site'),
    sessionCookie: getCookie(event, SESSION_COOKIE) !== undefined
  }, useRuntimeConfig(event).siteOrigin)

  if (refused) throw createError(refused)
})
