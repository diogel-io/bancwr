// SPIKE (diogel-io/bancwr#23). Sign-in from the browser, independent of the signer: NIP-07's
// window.nostr and a NIP-46 BunkerSigner both provide signEvent, and nothing else is needed.
// #11 builds the sign-in screen on this.

export interface EventTemplate {
  kind: number
  created_at: number
  tags: string[][]
  content: string
}

export interface SignedEvent extends EventTemplate {
  id: string
  pubkey: string
  sig: string
}

export interface NostrSigner {
  signEvent(template: EventTemplate): Promise<SignedEvent>
}

export type SignInResult =
  | { status: 200, pubkey: string, npub: string, role: string }
  | { status: 401, error: 'not_authenticated', reason?: string }
  | { status: 403, error: 'not_registered' | 'forbidden', npub?: string, reason?: string }

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), b => b.toString(16).padStart(2, '0')).join('')
}

function base64(text: string): string {
  let binary = ''
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte)
  return btoa(binary)
}

/**
 * 1. Ask for a single-use challenge. 2. Sign a NIP-98 event naming the login URL, POST, and the
 * SHA-256 of the exact body carrying the challenge. 3. Send it; the server sets the session cookie.
 * `origin` must be the origin the server is configured with (NUXT_SITE_ORIGIN).
 */
export async function signInWithNostr(
  signer: NostrSigner,
  origin: string,
  fetchImpl: typeof fetch = fetch
): Promise<SignInResult> {
  const issued = await fetchImpl(`${origin}/api/auth/challenge`, { method: 'POST', credentials: 'same-origin' })
  const { challenge } = await issued.json() as { challenge: string }

  const body = JSON.stringify({ challenge })
  const signed = await signer.signEvent({
    kind: 27235,
    created_at: Math.floor(Date.now() / 1000),
    content: '',
    tags: [['u', `${origin}/api/auth/login`], ['method', 'POST'], ['payload', await sha256Hex(body)]]
  })

  const response = await fetchImpl(`${origin}/api/auth/login`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json', 'Authorization': `Nostr ${base64(JSON.stringify(signed))}` },
    body
  })
  return { status: response.status, ...(await response.json()) } as SignInResult
}
