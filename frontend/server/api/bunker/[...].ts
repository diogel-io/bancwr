// Nitro server route to proxy all /api/bunker/* to backend
// Imported rather than left to Nitro's auto-import: nothing in this file's type context
// resolves auto-imports, so an explicit path keeps our own symbols checkable.
import { resolveBackendUrl, BackendTargetError } from '../../utils/backend'
import { proxyHeaders } from '../../utils/auth/proxy-signature'
import { sessionPubkey } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig()

  // No inline fallback: runtimeConfig already supplies the default, and two of them hide which
  // one is in force. resolveBackendUrl refuses a target that is this server — see #20. Its own
  // try/catch, because a misconfigured target is ours to explain, not a backend failure to
  // forward, and the catch below is shaped for $fetch errors.
  let backendUrl: string
  try {
    backendUrl = resolveBackendUrl(config.apiBase, {
      requestOrigin: getRequestURL(event).origin,
      selfPort: process.env.PORT,
    })
  } catch (error) {
    if (error instanceof BackendTargetError) {
      throw createError({ statusCode: 500, statusMessage: error.message })
    }

    throw error
  }

  // Only a signed-in session is proxied (#11). The bunker checks the role itself (#25), so a
  // signed-in user without the role for a route gets the bunker's 403, forwarded below.
  const pubkey = await sessionPubkey(event)
  if (!pubkey) {
    throw createError({ statusCode: 401, statusMessage: 'Not signed in', data: { error: 'not_authenticated' } })
  }

  // The request's own path and query, forwarded as they arrived: the signature covers this exact
  // string, and the bunker checks it against what it receives.
  const { pathname, search } = getRequestURL(event)
  const target = `${pathname}${search}`
  const url = `${backendUrl}${target}`

  // Forward the request
  const method = getMethod(event)
  const body = method !== 'GET' ? await readBody(event) : undefined

  try {
    return await $fetch(url, {
      method,
      body,
      headers: {
        'Content-Type': 'application/json',
        // Built here from the session, never copied from the browser's request.
        ...proxyHeaders(config.proxySecret, pubkey, method, target),
      },
    })
  } catch (error: any) {
    // Forward error response
    throw createError({
      statusCode: error.response?.status || 500,
      statusMessage: error.response?.statusText || 'Internal Server Error',
      data: error.response?._data,
    })
  }
})
