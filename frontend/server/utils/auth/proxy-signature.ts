// SPIKE (#23). How the Nitro proxy tells the bunker who is asking. The bunker trusts the pubkey
// only with a valid HMAC under the secret both share (NUXT_PROXY_SECRET here, BANCWR_PROXY_SECRET
// on the bunker), so a caller reaching the bunker's port directly cannot claim an identity.
import { createHmac } from 'node:crypto'

export const PROXY_HEADERS = {
  pubkey: 'x-bancwr-pubkey',
  timestamp: 'x-bancwr-timestamp',
  signature: 'x-bancwr-signature'
} as const

/** `v1\n<timestamp>\n<METHOD>\n<path?query>\n<pubkey hex>`, HMAC-SHA256, lowercase hex. */
export function proxySignature(secret: string, input: {
  timestamp: number
  method: string
  path: string
  pubkey: string
}): string {
  const canonical = ['v1', String(input.timestamp), input.method.toUpperCase(), input.path, input.pubkey].join('\n')
  return createHmac('sha256', secret).update(canonical, 'utf8').digest('hex')
}
