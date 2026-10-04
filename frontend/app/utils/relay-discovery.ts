// Known relays for the bunker relay search (#78), from NIP-66 relay discovery events (kind 30166)
// that relay monitors publish. Read from the console's indexer relays; when no monitor answers, a
// short built-in list stands in. These are unauthenticated hints for the administrator to choose
// from: nothing here decides anything, and every relay chosen is validated by the bunker.
import type { NostrEvent } from 'nostr-tools/pure'
import { normalizeRelayUrl, relayKey } from '~/utils/relay-url'

/** NIP-66 relay discovery. */
export const RELAY_DISCOVERY_KIND = 30166

/** How many discovery events one search asks for: monitors publish one per relay they watch. */
export const DISCOVERY_LIMIT = 500

export interface KnownRelay {
  /** Normalised, as the bunker stores it. */
  url: string
  /** NIP-66 `rtt-open`: how long the monitor took to open a connection, in milliseconds. */
  rttOpen?: number
  /** From the `R` tags: true for `auth`, false for `!auth`; unknown when the monitor did not say. */
  auth?: boolean
  payment?: boolean
  /** The newest report's `created_at`, when it came from a monitor. */
  seenAt?: number
}

/**
 * Shown when no monitor answers. Long-running public relays that accept NIP-46 traffic without
 * payment; the administrator still chooses, and can add any other by address.
 */
export const FALLBACK_RELAYS: readonly KnownRelay[] = [
  { url: 'wss://relay.nsec.app' },
  { url: 'wss://relay.damus.io' },
  { url: 'wss://nos.lol' },
  { url: 'wss://relay.primal.net' },
  { url: 'wss://offchain.pub' }
]

/** A requirement from `R` tags: `name` is true, `!name` false; the last one given wins. */
function requirement(tags: string[][], name: string): boolean | undefined {
  let value: boolean | undefined
  for (const tag of tags) {
    if (tag[0] !== 'R') continue
    if (tag[1] === name) value = true
    else if (tag[1] === `!${name}`) value = false
  }
  return value
}

/**
 * One discovery event as a known relay, or undefined when it is not one: another kind, or a `d`
 * tag that is not a ws(s):// address (NIP-66 allows a pubkey for relays without a URL).
 */
export function parseRelayDiscovery(event: Pick<NostrEvent, 'kind' | 'tags' | 'created_at'>): KnownRelay | undefined {
  if (event.kind !== RELAY_DISCOVERY_KIND) return undefined
  const d = event.tags.find(tag => tag[0] === 'd')?.[1]
  if (!d) return undefined
  const normalized = normalizeRelayUrl(d)
  if ('error' in normalized) return undefined
  const rtt = Number(event.tags.find(tag => tag[0] === 'rtt-open')?.[1])
  return {
    url: normalized.url,
    rttOpen: Number.isFinite(rtt) && rtt >= 0 ? Math.round(rtt) : undefined,
    auth: requirement(event.tags, 'auth'),
    payment: requirement(event.tags, 'payment'),
    seenAt: event.created_at
  }
}

/**
 * Discovery events as one entry per relay, keeping each relay's newest report, fastest to open
 * first and relays without a time last.
 */
export function knownRelays(events: Pick<NostrEvent, 'kind' | 'tags' | 'created_at'>[]): KnownRelay[] {
  const newest = new Map<string, KnownRelay>()
  for (const event of events) {
    const relay = parseRelayDiscovery(event)
    if (!relay) continue
    const key = relayKey(relay.url)
    const held = newest.get(key)
    if (!held || (relay.seenAt ?? 0) > (held.seenAt ?? 0)) newest.set(key, relay)
  }
  return [...newest.values()].sort((a, b) =>
    (a.rttOpen ?? Infinity) - (b.rttOpen ?? Infinity) || a.url.localeCompare(b.url))
}

/** The relays whose address contains `text`, ignoring case and the scheme. */
export function filterRelays(relays: readonly KnownRelay[], text: string): KnownRelay[] {
  const needle = text.trim().toLowerCase().replace(/^wss?:\/\//u, '')
  if (!needle) return [...relays]
  return relays.filter(relay => relay.url.toLowerCase().includes(needle))
}

/** Whether the bunker accepts this address: wss://, or ws:// on this machine only. */
export function isBunkerRelayUrl(url: string): boolean {
  try {
    const { protocol, hostname } = new URL(url)
    return protocol === 'wss:' || (protocol === 'ws:' && ['localhost', '127.0.0.1', '[::1]'].includes(hostname))
  } catch {
    return false
  }
}
