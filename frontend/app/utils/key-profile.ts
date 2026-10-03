// A key's name and picture, to show whose key is about to sign in (#70). Best effort and bounded:
// one kind 0 lookup on the default and indexer relays, given up after a few seconds, never
// required for anything. Browser-only (README, "Nostr (nostr-tools)").
import type { NostrEvent } from 'nostr-tools/pure'
import type { Filter } from 'nostr-tools/filter'
import { parseFollowProfile, type FollowProfile } from '~/utils/follows'
import { newestEvent } from '~/utils/profile'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS, parseRelayList } from '~/utils/profile-relays'

export const KEY_PROFILE_TIMEOUT_MS = 3000

export type QueryEvents = (relays: string[], filter: Filter, maxWait: number) => Promise<NostrEvent[]>

/** The relays to look a key up on: the configured (or default) profile relays and indexers. */
export function lookupRelays(profileRelays: unknown, indexerRelays: unknown): string[] {
  const profile = parseRelayList(profileRelays as string | string[] | undefined)
  const indexers = parseRelayList(indexerRelays as string | string[] | undefined)
  return [...new Set([...(profile.length ? profile : DEFAULT_PROFILE_RELAYS), ...(indexers.length ? indexers : DEFAULT_INDEXER_RELAYS)])]
}

/** The key's newest profile, or undefined if none arrives within `timeoutMs`. Never throws. */
export async function lookupKeyProfile(pubkey: string, relays: string[], query: QueryEvents, timeoutMs = KEY_PROFILE_TIMEOUT_MS): Promise<FollowProfile | undefined> {
  const timeout = new Promise<undefined>(resolve => setTimeout(() => resolve(undefined), timeoutMs))
  const lookup = query(relays, { kinds: [0], authors: [pubkey] }, timeoutMs)
    .then((events) => {
      const newest = newestEvent(events.filter(e => e.kind === 0 && e.pubkey === pubkey))
      return newest ? parseFollowProfile(newest) : undefined
    })
    .catch(() => undefined)
  return Promise.race([lookup, timeout])
}

/** The real query, over a SimplePool that is closed afterwards. */
export async function queryRelays(relays: string[], filter: Filter, maxWait: number): Promise<NostrEvent[]> {
  const { SimplePool } = await import('nostr-tools/pool')
  const pool = new SimplePool()
  try {
    return await pool.querySync(relays, filter, { maxWait })
  } finally {
    pool.close(relays)
  }
}
