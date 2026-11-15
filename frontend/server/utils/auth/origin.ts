// Which requests the console's server accepts from a browser. A request that changes something
// must come from the console's own origin, NUXT_SITE_ORIGIN: the session cookie alone is not enough,
// because SameSite=Strict lets every origin on the same site send it. A body forwarded to the bunker
// must be declared JSON, so a plain HTML form can never produce one.

/** Methods that change nothing, and so need no origin check. */
const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS'])

export interface IncomingRequest {
  method: string
  /** The request's path, without the query. */
  path: string
  /** The `Origin` header, if any. */
  origin?: string
  /** The `Sec-Fetch-Site` header, if any. */
  secFetchSite?: string
  /** The `Content-Type` header, if any. */
  contentType?: string
  /** Whether the request carries a body: a non-zero `Content-Length`, or `Transfer-Encoding`. */
  hasBody: boolean
}

export interface Refusal {
  statusCode: 403 | 415
  statusMessage: string
  data: { error: string, reason?: string }
}

const BAD_ORIGIN: Refusal = {
  statusCode: 403,
  statusMessage: 'Forbidden',
  data: { error: 'forbidden', reason: 'bad_origin' }
}

const NOT_JSON: Refusal = {
  statusCode: 415,
  statusMessage: 'Unsupported Media Type',
  data: { error: 'unsupported_media_type' }
}

/**
 * Why the request is refused, or undefined to let it through.
 *
 * A browser sends `Origin` with every request that is not GET or HEAD, and `Sec-Fetch-Site` with
 * every request, so a request carrying neither did not come from a browser page and cannot be one
 * a page made on someone's behalf. Those are allowed, for operators' scripts and health checks.
 */
export function refusal(request: IncomingRequest, siteOrigin: string): Refusal | undefined {
  if (SAFE_METHODS.has(request.method.toUpperCase())) return undefined

  if (request.origin !== undefined) {
    if (request.origin !== siteOrigin) return BAD_ORIGIN
  } else if (request.secFetchSite !== undefined && request.secFetchSite !== 'same-origin') {
    return BAD_ORIGIN
  }

  if (request.hasBody && request.path.toLowerCase().startsWith('/api/bunker/')) {
    const mediaType = request.contentType?.split(';')[0]?.trim().toLowerCase()
    if (mediaType !== 'application/json') return NOT_JSON
  }

  return undefined
}
