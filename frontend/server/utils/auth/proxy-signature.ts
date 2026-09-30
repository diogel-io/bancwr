// How the frontend tells the bunker who is calling (#25). The bunker trusts an identity only with
// a valid HMAC under the secret both hold (NUXT_PROXY_SECRET here, BANCWR_PROXY_SECRET there), so
// a caller reaching the bunker directly cannot claim to be anyone. backend/src/proxy_auth.rs
// verifies it; tests/server/auth/proxy-signature.spec.ts and the backend's proxy_auth_test.rs
// assert the same vectors. Sign-in (#11) signs every proxied request with the session's pubkey.
import { createHmac } from 'node:crypto'

export const PROXY_HEADERS = {
  identity: 'x-bancwr-identity',
  timestamp: 'x-bancwr-timestamp',
  signature: 'x-bancwr-signature'
} as const

/** The frontend acting for itself, before anyone is signed in. */
export const SERVICE_IDENTITY = 'service'

/** A signed-in user's lowercase hex pubkey, or the service identity. */
export type ProxyIdentity = string

/** `v1\n<timestamp>\n<METHOD>\n<path?query>\n<identity>`, HMAC-SHA256, lowercase hex. */
export function proxySignature(secret: string, input: {
  timestamp: number
  method: string
  path: string
  identity: ProxyIdentity
}): string {
  const canonical = ['v1', String(input.timestamp), input.method.toUpperCase(), input.path, input.identity].join('\n')
  return createHmac('sha256', secret).update(canonical, 'utf8').digest('hex')
}

/** The three headers for one request. `path` includes the query string. */
export function proxyHeaders(secret: string, identity: ProxyIdentity, method: string, path: string): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000)
  return {
    [PROXY_HEADERS.identity]: identity,
    [PROXY_HEADERS.timestamp]: String(timestamp),
    [PROXY_HEADERS.signature]: proxySignature(secret, { timestamp, method, path, identity })
  }
}
