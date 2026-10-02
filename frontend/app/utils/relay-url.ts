// Relay URL rules for the relay list (#33), Porwr's relay-url.ts, retyped: ws:// and wss:// only,
// no fragment (the WebSocket constructor throws on one), a real hostname, at most 255 characters,
// the host lowercased and a trailing slash on an empty path dropped, as NIP-65's examples are
// written. Comparing two URLs goes through nostr-tools' normalizeURL.
import { normalizeURL } from 'nostr-tools/utils'

export type RelayUrlResult = { url: string, hostname: string } | { error: string }

export const MAX_RELAY_URL_LENGTH = 255

export function normalizeRelayUrl(input: string): RelayUrlResult {
  const trimmed = input.trim()
  if (!trimmed) return { error: 'Enter a relay address.' }
  let url: URL
  try {
    url = new URL(trimmed)
  } catch {
    return { error: 'This is not a valid address. Relay addresses look like wss://relay.example.com' }
  }
  if (url.protocol !== 'ws:' && url.protocol !== 'wss:') return { error: 'Relay addresses start with wss:// (or ws://).' }
  if (!url.hostname || url.hostname.startsWith('.')) return { error: 'The address needs a valid hostname.' }
  if (url.hash) return { error: 'A relay address cannot contain a #fragment.' }
  let normalized = url.toString()
  if (url.pathname === '/' && !url.search && normalized.endsWith('/')) normalized = normalized.slice(0, -1)
  if (normalized.length > MAX_RELAY_URL_LENGTH) return { error: `Relay addresses can be up to ${MAX_RELAY_URL_LENGTH} characters.` }
  return { url: normalized, hostname: url.hostname }
}

/** One key per relay, so `wss://x` and `wss://x/` are the same relay. */
export function relayKey(url: string): string {
  try {
    return normalizeURL(url.trim())
  } catch {
    return url.trim().toLowerCase()
  }
}

export function sameRelay(a: string, b: string): boolean {
  return relayKey(a) === relayKey(b)
}

/** An unencrypted ws:// relay that is not on this machine or network. */
export function isInsecureRemote(url: string): boolean {
  try {
    const { protocol, hostname } = new URL(url)
    return protocol === 'ws:' && !['localhost', '127.0.0.1', '[::1]'].includes(hostname) && !hostname.endsWith('.local')
  } catch {
    return false
  }
}
