// Which relays the profile is read from and published to (#30, decided 2026-10-01): the member's
// own NIP-65 write relays (kind 10002) plus the deployment's defaults (NUXT_PUBLIC_PROFILE_RELAYS),
// plus indexer relays (NUXT_PUBLIC_INDEXER_RELAYS, #62). Indexers collect profiles and relay lists
// from across the network: a member whose list and profile are on none of their own or the default
// relays is found there. #33 takes over editing the member's list.
import { normalizeURL } from 'nostr-tools/utils'
import type { NostrEvent } from 'nostr-tools/pure'

/** Used when NUXT_PUBLIC_PROFILE_RELAYS is unset or empty. Self-hosters can choose their own. */
export const DEFAULT_PROFILE_RELAYS = ['wss://relay.damus.io/', 'wss://nos.lol/', 'wss://relay.primal.net/']

/**
 * Used when NUXT_PUBLIC_INDEXER_RELAYS is unset or empty. Each held kind 0 and kind 10002 for the
 * keys probed on 2026-10-02 (#62); any one answering is enough.
 */
export const DEFAULT_INDEXER_RELAYS = ['wss://purplepag.es/', 'wss://profiles.nostr1.com/', 'wss://relay.nos.social/']

/** Used when NUXT_PUBLIC_BLOSSOM_SERVER is unset or empty: Porwr's default. */
export const DEFAULT_BLOSSOM_SERVER = 'https://blossom.primal.net'

/** A bound on how many of the member's own relays are used, so a huge list cannot fan out. */
export const MAX_OWN_RELAYS = 8

function relayUrl(value: string | undefined): string | undefined {
  if (!value || !/^wss?:\/\//iu.test(value.trim())) return undefined
  try {
    return normalizeURL(value.trim())
  } catch {
    return undefined
  }
}

/** The configured defaults, from a comma list. */
export function parseRelayList(value: string | string[] | undefined): string[] {
  const items = Array.isArray(value) ? value : (value ?? '').split(',')
  return unique(items.map(relayUrl))
}

/** The write relays of a NIP-65 list: `r` tags marked `write` or not marked at all. */
export function writeRelays(list: Pick<NostrEvent, 'tags'> | undefined): string[] {
  if (!list) return []
  const urls = list.tags
    .filter(tag => tag[0] === 'r' && (tag[2] === undefined || tag[2] === 'write'))
    .map(tag => relayUrl(tag[1]))
  return unique(urls).slice(0, MAX_OWN_RELAYS)
}

/**
 * Where the profile is read from and published to: the member's write relays first, then the
 * defaults, then any others (indexers, a relay the member asked to search), without duplicates.
 */
export function profileRelays(defaults: string[], list: Pick<NostrEvent, 'tags'> | undefined, others: string[] = []): string[] {
  return unique([...writeRelays(list), ...defaults, ...others])
}

/** The one relay URL in `value`, normalised, or undefined if it is not a ws(s):// URL. */
export function parseRelayUrl(value: string): string | undefined {
  return relayUrl(value)
}

/** The first server of a BUD-03 server list (kind 10063), if any. */
export function blossomServerFrom(list: Pick<NostrEvent, 'tags'> | undefined): string | undefined {
  const server = list?.tags.find(tag => tag[0] === 'server' && /^https:\/\//iu.test(tag[1] ?? ''))?.[1]
  return server ? server.replace(/\/+$/u, '') : undefined
}

function unique(urls: (string | undefined)[]): string[] {
  return [...new Set(urls.filter((url): url is string => !!url))]
}
