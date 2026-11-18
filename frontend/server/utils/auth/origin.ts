// Which requests the console's server accepts from a browser. A request that changes something
// must come from the console's own origin, NUXT_SITE_ORIGIN: the session cookie alone is not enough,
// because SameSite=Strict lets every origin on the same site send it. A body forwarded to the bunker
// must be declared JSON, so a plain HTML form can never produce one; the proxy enforces that on
// what it forwards (see jsonBodyRefusal).

/** Methods that change nothing, and so need no origin check. */
const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS'])

/** Whether a request with this method changes nothing, and so needs no origin check. */
export function isSafeMethod(method: string): boolean {
  return SAFE_METHODS.has(method.toUpperCase())
}

export interface IncomingRequest {
  method: string
  /** The `Origin` header, if any. */
  origin?: string
  /** The `Sec-Fetch-Site` header, if any. */
  secFetchSite?: string
  /** Whether the request carries the session cookie, whatever its value. */
  sessionCookie: boolean
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
 * Why the request is refused for where it came from, or undefined to let it through.
 *
 * `Origin` must be the console's own. With no `Origin`, `Sec-Fetch-Site` must be `same-origin`.
 * A request carrying neither is let through only when it also carries no session cookie: then it
 * acts as nobody, as an operator's script or a health check does. With the cookie and neither
 * header, nothing shows the console's own page sent it, so it is refused.
 */
export function refusal(request: IncomingRequest, siteOrigin: string): Refusal | undefined {
  if (isSafeMethod(request.method)) return undefined

  if (request.origin !== undefined) {
    return request.origin === siteOrigin ? undefined : BAD_ORIGIN
  }
  if (request.secFetchSite !== undefined) {
    return request.secFetchSite === 'same-origin' ? undefined : BAD_ORIGIN
  }
  return request.sessionCookie ? BAD_ORIGIN : undefined
}

/** Whether a request carries a body: a `Content-Length` other than 0, or any `Transfer-Encoding`. */
export function hasBody(contentLength: string | undefined, transferEncoding: string | undefined): boolean {
  return (contentLength !== undefined && contentLength !== '0') || transferEncoding !== undefined
}

/** The media type of a `Content-Type` header, without parameters, in lower case. */
function mediaType(contentType: string | undefined): string | undefined {
  return contentType?.split(';')[0]?.trim().toLowerCase()
}

/** Refuses a body that is not declared `application/json`, or undefined when there is no body. */
export function jsonBodyRefusal(body: { hasBody: boolean, contentType?: string }): Refusal | undefined {
  if (!body.hasBody) return undefined
  return mediaType(body.contentType) === 'application/json' ? undefined : NOT_JSON
}
