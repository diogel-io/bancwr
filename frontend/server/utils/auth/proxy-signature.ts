// How the frontend tells the bunker who is calling (#25). The bunker trusts an identity only with
// a valid HMAC under the secret both hold (NUXT_PROXY_SECRET here, BANCWR_PROXY_SECRET there), so
// a caller reaching the bunker directly cannot claim to be anyone. backend/src/proxy_auth.rs
// verifies it; tests/server/auth/proxy-signature.spec.ts and the backend's proxy_auth_test.rs
// assert the same vectors. Sign-in (#11) signs every proxied request with the session's pubkey.
// The signature covers the body and a fresh nonce too, and the bunker accepts each nonce once, so
// a request seen between the two cannot be sent again, or with another body.
import { createHash, createHmac, randomBytes } from 'node:crypto'

export const PROXY_HEADERS = {
  identity: 'x-bancwr-identity',
  timestamp: 'x-bancwr-timestamp',
  nonce: 'x-bancwr-nonce',
  signature: 'x-bancwr-signature'
} as const

/** The frontend acting for itself, before anyone is signed in. */
export const SERVICE_IDENTITY = 'service'

/** A signed-in user's lowercase hex pubkey, or the service identity. */
export type ProxyIdentity = string

/**
 * `v2\n<timestamp>\n<nonce>\n<METHOD>\n<path?query>\n<identity>\n<hex SHA-256 of the body>`,
 * HMAC-SHA256, lowercase hex. `body` is the exact string sent, empty for none.
 */
export function proxySignature(secret: string, input: {
  timestamp: number
  nonce: string
  method: string
  path: string
  identity: ProxyIdentity
  body: string
}): string {
  const bodyHash = createHash('sha256').update(input.body, 'utf8').digest('hex')
  const canonical = ['v2', String(input.timestamp), input.nonce, input.method.toUpperCase(), input.path, input.identity, bodyHash].join('\n')
  return createHmac('sha256', secret).update(canonical, 'utf8').digest('hex')
}

/** The four headers for one request. `path` includes the query string; `body` is the exact string sent. */
export function proxyHeaders(secret: string, identity: ProxyIdentity, method: string, path: string, body = ''): Record<string, string> {
  const timestamp = Math.floor(Date.now() / 1000)
  const nonce = randomBytes(16).toString('hex')
  return {
    [PROXY_HEADERS.identity]: identity,
    [PROXY_HEADERS.timestamp]: String(timestamp),
    [PROXY_HEADERS.nonce]: nonce,
    [PROXY_HEADERS.signature]: proxySignature(secret, { timestamp, nonce, method, path, identity, body })
  }
}
