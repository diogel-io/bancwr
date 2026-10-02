// NIP-05 identifier verification (#30): Porwr's nip05-service.ts behaviour, retyped. The page runs
// it only when the member asks (never on input), from the browser; NIP-05 requires servers to
// allow that with CORS.

export type Nip05Status = 'verified' | 'malformed' | 'network-error' | 'invalid-response' | 'not-found' | 'pubkey-mismatch'

export interface Nip05Result {
  status: Nip05Status
  identifier: string
  domain?: string
}

export interface ParsedNip05 {
  name: string
  domain: string
}

/** How long a domain gets to answer. */
export const NIP05_TIMEOUT_MS = 8000

export function parseNip05Identifier(identifier: string): ParsedNip05 | null {
  const match = /^(?<name>[^@\s]+)@(?<domain>[^@\s]+)$/u.exec(identifier.trim())
  const name = match?.groups?.name
  const domain = match?.groups?.domain
  if (!name || !domain) return null
  try {
    const url = new URL(`https://${domain}`)
    // A bare word is not a domain, and a port or path would change where the request goes.
    if (!url.hostname.includes('.') || url.hostname !== domain.toLowerCase()) return null
  } catch {
    return null
  }
  return { name, domain }
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return typeof value === 'object' && value !== null && Object.values(value).every(entry => typeof entry === 'string')
}

/** Asks the identifier's domain which key it maps the name to, and compares it with `pubkey` (hex). */
export async function verifyNip05(identifier: string, pubkey: string, fetcher: typeof fetch = fetch): Promise<Nip05Result> {
  const trimmed = identifier.trim()
  const parsed = parseNip05Identifier(trimmed)
  if (!parsed) return { status: 'malformed', identifier: trimmed }
  const result = (status: Nip05Status): Nip05Result => ({ status, identifier: trimmed, domain: parsed.domain })

  const url = new URL(`https://${parsed.domain}/.well-known/nostr.json`)
  url.searchParams.set('name', parsed.name)

  let body: unknown
  try {
    const response = await fetcher(url.toString(), {
      headers: { Accept: 'application/json' },
      // No redirects: NIP-05 forbids them, and following one would verify against another domain.
      redirect: 'error',
      signal: AbortSignal.timeout(NIP05_TIMEOUT_MS)
    })
    if (!response.ok) return result(response.status === 404 ? 'not-found' : 'network-error')
    body = await response.json()
  } catch (error) {
    return result(error instanceof SyntaxError ? 'invalid-response' : 'network-error')
  }

  const names = typeof body === 'object' && body !== null ? (body as { names?: unknown }).names : undefined
  if (!isStringRecord(names)) return result('invalid-response')
  const found = names[parsed.name] ?? names[parsed.name.toLowerCase()]
  if (!found) return result('not-found')
  return result(found.toLowerCase() === pubkey.toLowerCase() ? 'verified' : 'pubkey-mismatch')
}
