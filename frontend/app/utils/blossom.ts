// Uploading a profile image to a Blossom server (BUD-01/02, #30), authorised by a kind 24242 event
// the member's own signer signs. Browser-only: it uses fetch and Web Crypto from the page.
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

/** How long the upload authorisation is valid for. */
export const AUTH_LIFETIME_S = 300

export interface BlobDescriptor {
  url: string
  sha256: string
  size?: number
  type?: string
}

export class BlossomError extends Error {}

export async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes)
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('')
}

/** The BUD-02 upload authorisation for one blob. */
export function uploadAuthTemplate(sha256: string, now = Math.floor(Date.now() / 1000)): EventTemplate {
  return {
    kind: 24242,
    created_at: now,
    content: 'Upload profile image',
    tags: [['t', 'upload'], ['x', sha256], ['expiration', String(now + AUTH_LIFETIME_S)]]
  }
}

export function authorizationHeader(event: SignedEvent): string {
  const bytes = new TextEncoder().encode(JSON.stringify(event))
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return `Nostr ${btoa(binary)}`
}

function isDescriptor(value: unknown): value is BlobDescriptor {
  const record = value as Record<string, unknown> | null
  return typeof record === 'object' && record !== null && typeof record.url === 'string' && typeof record.sha256 === 'string'
}

/**
 * Uploads `blob` to `server` and returns the URL to put in the profile. The server's answer is
 * checked: its hash must be the one signed for, and its URL must be https.
 */
export async function uploadToBlossom(server: string, blob: Blob, signer: NostrSigner, fetcher: typeof fetch = fetch): Promise<string> {
  const bytes = await blob.arrayBuffer()
  const sha256 = await sha256Hex(bytes)

  let event: SignedEvent
  try {
    event = await signer.signEvent(uploadAuthTemplate(sha256))
  } catch {
    throw new BlossomError('Your signer did not sign the upload. Try again, and approve it.')
  }

  let response: Response
  try {
    response = await fetcher(`${server.replace(/\/+$/u, '')}/upload`, {
      method: 'PUT',
      headers: { 'Authorization': authorizationHeader(event), 'Content-Type': blob.type || 'application/octet-stream' },
      body: blob
    })
  } catch {
    throw new BlossomError(`Could not reach ${new URL(server).host}. It may be down, or not allow uploads from a browser.`)
  }
  if (!response.ok) {
    // BUD-01: the reason, if any, is in X-Reason.
    const reason = response.headers.get('X-Reason')
    throw new BlossomError(`${new URL(server).host} refused the upload${reason ? `: ${reason}` : ` (HTTP ${response.status})`}.`)
  }

  const descriptor: unknown = await response.json().catch(() => undefined)
  if (!isDescriptor(descriptor) || descriptor.sha256.toLowerCase() !== sha256) {
    throw new BlossomError('The server\'s answer did not match the uploaded image.')
  }
  if (!descriptor.url.startsWith('https://')) {
    throw new BlossomError('The server returned an address that is not https.')
  }
  return descriptor.url
}
