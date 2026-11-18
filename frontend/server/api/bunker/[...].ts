// Nitro server route to proxy all /api/bunker/* to backend
// Imported rather than left to Nitro's auto-import: nothing in this file's type context
// resolves auto-imports, so an explicit path keeps our own symbols checkable.
import { resolveBackendUrl, BackendTargetError } from '../../utils/backend'
import { hasBody, jsonBodyRefusal } from '../../utils/auth/origin'
import { proxyHeaders } from '../../utils/auth/proxy-signature'
import { sessionPubkey } from '../../utils/auth/session'
import { downloadHeaders, isDownload } from '../../utils/download'

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig()
  const method = getMethod(event)

  // A body is forwarded only as JSON, so a plain HTML form can never produce one. Checked here, on
  // the request this handler forwards, so the rule holds however the router matched the path.
  const contentType = getHeader(event, 'content-type')
  const forwardsBody = method !== 'GET'
    && hasBody(getHeader(event, 'content-length'), getHeader(event, 'transfer-encoding'))
  const notJson = forwardsBody ? jsonBodyRefusal({ hasBody: true, contentType }) : undefined
  if (notJson) throw createError(notJson)

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
  const body = forwardsBody ? await readBody(event) : undefined

  // A download (#96) is streamed through rather than parsed, keeping only its content type and
  // file name. A refusal is JSON, forwarded as any other error is.
  if (method === 'GET' && isDownload(pathname)) {
    const response = await fetch(url, {
      headers: proxyHeaders(config.proxySecret, pubkey, method, target),
      redirect: 'error'
    })
    if (!response.ok || !response.body) {
      const data = await response.json().catch(() => undefined)
      throw createError({ statusCode: response.status || 500, statusMessage: response.statusText || 'Internal Server Error', data })
    }
    setResponseHeaders(event, downloadHeaders(response.headers))
    return sendStream(event, response.body)
  }

  try {
    return await $fetch(url, {
      method,
      body,
      headers: {
        // The type the browser declared, checked above to be JSON; none when there is no body.
        ...(forwardsBody && contentType ? { 'Content-Type': contentType } : {}),
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
