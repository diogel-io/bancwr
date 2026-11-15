// Refuses a request that changes something unless it comes from the console's own origin, and a
// body for the bunker unless it is JSON. See utils/auth/origin.ts. It runs before every route, the
// sign-in routes and the proxy to the bunker included.
import { refusal } from '../utils/auth/origin'

export default defineEventHandler((event) => {
  const contentLength = getHeader(event, 'content-length')
  const refused = refusal({
    method: event.method,
    path: getRequestURL(event).pathname,
    origin: getHeader(event, 'origin'),
    secFetchSite: getHeader(event, 'sec-fetch-site'),
    contentType: getHeader(event, 'content-type'),
    hasBody: (contentLength !== undefined && contentLength !== '0') || getHeader(event, 'transfer-encoding') !== undefined
  }, useRuntimeConfig(event).siteOrigin)

  if (refused) throw createError(refused)
})
