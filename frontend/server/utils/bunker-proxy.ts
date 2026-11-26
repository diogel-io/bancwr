// The proxy from the browser to a bunker (#11, #25), shared by /api/bunker/* (the default bunker)
// and, SPIKE (#151), /api/bunkers/<id>/* (any configured bunker). It requires a session, signs the
// session's pubkey with that bunker's own secret, and sends the member's session on that bunker
// when the bunker checks each member's own sign-in (member proof).
import type { H3Event } from 'h3'
import { hasBody, jsonBodyRefusal } from './auth/origin'
import { proxyHeaders } from './auth/proxy-signature'
import { liveSession } from './auth/session'
import { downloadHeaders, isDownload } from './download'
import { BackendTargetError, resolveBackendUrl } from './backend'
import type { BunkerTarget } from './bunkers'

const MEMBER_SESSION_HEADER = 'x-bancwr-member-session'

/** Proxies this request to `bunker`, as `target` (the bunker's own path and query). */
export async function proxyToBunker(event: H3Event, bunker: BunkerTarget, target: string) {
  const method = getMethod(event)

  // A body is forwarded only as JSON, so a plain HTML form can never produce one.
  const contentType = getHeader(event, 'content-type')
  const forwardsBody = method !== 'GET'
    && hasBody(getHeader(event, 'content-length'), getHeader(event, 'transfer-encoding'))
  const notJson = forwardsBody ? jsonBodyRefusal({ hasBody: true, contentType }) : undefined
  if (notJson) throw createError(notJson)

  // resolveBackendUrl refuses a target that is this server (#20).
  let backendUrl: string
  try {
    backendUrl = resolveBackendUrl(bunker.origin, { requestOrigin: getRequestURL(event).origin, selfPort: process.env.PORT })
  } catch (error) {
    if (error instanceof BackendTargetError) throw createError({ statusCode: 500, statusMessage: error.message })
    throw error
  }

  // Only a signed-in session is proxied (#11). The bunker checks the role itself (#25).
  const session = await liveSession(event)
  if (!session) {
    throw createError({ statusCode: 401, statusMessage: 'Not signed in', data: { error: 'not_authenticated' } })
  }
  const memberSession = session.bunkers?.[bunker.id]

  const url = `${backendUrl}${target}`
  // The exact string that arrived: the signature covers its hash.
  const body = forwardsBody ? (await readRawBody(event, 'utf8')) ?? '' : ''
  const headers = (withBody: boolean) => ({
    ...(withBody && forwardsBody && contentType ? { 'Content-Type': contentType } : {}),
    ...proxyHeaders(bunker.secret, session.pubkey, method, target, body),
    ...(memberSession ? { [MEMBER_SESSION_HEADER]: memberSession } : {})
  })

  if (method === 'GET' && isDownload(target.split('?')[0]!)) {
    const response = await fetch(url, { headers: headers(false), redirect: 'error' })
    if (!response.ok || !response.body) {
      const data = await response.json().catch(() => undefined)
      throw createError({ statusCode: response.status || 500, statusMessage: response.statusText || 'Internal Server Error', data })
    }
    setResponseHeaders(event, downloadHeaders(response.headers))
    return sendStream(event, response.body)
  }

  try {
    return await $fetch(url, { method, body: body || undefined, headers: headers(true) })
  } catch (error: any) {
    throw createError({
      statusCode: error.response?.status || 500,
      statusMessage: error.response?.statusText || 'Internal Server Error',
      data: error.response?._data
    })
  }
}
