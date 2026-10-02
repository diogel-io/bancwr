// The NIP-02 follow list (kind 3) (#32), read at nips commit 0046368a (2026-09-27):
// `["p", <32-byte hex key>, <relay URL>, <petname>]` tags; `.content` is not used by NIP-02, but
// some clients store a relay map there. Kind 3 is replaceable, so a publish replaces the whole
// list: everything here exists so that nothing the member did not remove is lost. Porwr's
// contact-list-service.ts behaviour, retyped, without its two gaps (first event to arrive instead
// of the newest; republishing only the `p` tags with empty content).
import { decode, npubEncode } from 'nostr-tools/nip19'
import type { NostrEvent } from 'nostr-tools/pure'
import { parseProfileContent } from '~/utils/profile'

const HEX_KEY = /^[0-9a-f]{64}$/iu

/** A followed key and the `p` tag it came from, kept as it was (relay hint, petname). */
export interface Follow {
  pubkey: string
  tag: string[]
}

/** What a follow is shown with, from that key's kind 0. */
export interface FollowProfile {
  name?: string
  picture?: string
  nip05?: string
  createdAt: number
}

/** Lowercase hex for a hex key (any case) or an npub; null for anything else. */
export function normalizePubkey(input: string): string | null {
  const value = input.trim()
  if (HEX_KEY.test(value)) return value.toLowerCase()
  try {
    const decoded = decode(value)
    return decoded.type === 'npub' && HEX_KEY.test(decoded.data) ? decoded.data.toLowerCase() : null
  } catch {
    return null
  }
}

/** The follows in a kind 3, first occurrence per key, each with its tag unchanged. */
export function parseFollows(event: Pick<NostrEvent, 'tags'> | undefined): Follow[] {
  const seen = new Set<string>()
  const follows: Follow[] = []
  for (const tag of event?.tags ?? []) {
    if (tag[0] !== 'p' || !tag[1]) continue
    const pubkey = normalizePubkey(tag[1])
    if (!pubkey || seen.has(pubkey)) continue
    seen.add(pubkey)
    follows.push({ pubkey, tag })
  }
  return follows
}

/**
 * The tags and content to publish: the latest list as it stands, without the `p` tags of removed
 * keys, with added keys appended. Every other tag (`t`, `a`, anything) and every untouched `p` tag
 * keeps its place and its relay hint and petname; `content` is kept as it was.
 */
export function applyChanges(latest: Pick<NostrEvent, 'tags' | 'content'> | undefined, adds: string[], removes: string[]): { tags: string[][], content: string } {
  const removed = new Set(removes)
  const tags = (latest?.tags ?? []).filter((tag) => {
    if (tag[0] !== 'p' || !tag[1]) return true
    const pubkey = normalizePubkey(tag[1])
    return !pubkey || !removed.has(pubkey)
  })
  const present = new Set(parseFollows({ tags }).map(f => f.pubkey))
  for (const pubkey of adds) {
    if (!present.has(pubkey)) {
      tags.push(['p', pubkey])
      present.add(pubkey)
    }
  }
  return { tags, content: latest?.content ?? '' }
}

/**
 * Whether `next` differs from `latest` by exactly the staged changes: the keys followed are the
 * latest's, minus `removes`, plus `adds`, and no other tag changed. A last check before signing.
 */
export function onlyStagedChanges(latest: Pick<NostrEvent, 'tags' | 'content'> | undefined, next: { tags: string[][], content: string }, adds: string[], removes: string[]): boolean {
  const before = new Set(parseFollows(latest).map(f => f.pubkey))
  const after = new Set(parseFollows(next).map(f => f.pubkey))
  const expected = new Set([...before].filter(k => !removes.includes(k)))
  for (const key of adds) expected.add(key)
  if (after.size !== expected.size || [...expected].some(k => !after.has(k))) return false
  const others = (tags: string[][]) => JSON.stringify(tags.filter(t => t[0] !== 'p'))
  return others(latest?.tags ?? []) === others(next.tags) && (latest?.content ?? '') === next.content
}

export type FollowInputResult = { pubkey: string } | { error: string }

/** A key to follow from hex or npub input, or why it cannot be. NIP-05 is resolved separately. */
export function validateFollowInput(input: string, following: Set<string>, self: string): FollowInputResult {
  const pubkey = normalizePubkey(input)
  if (!pubkey) return { error: 'Enter an npub, a 64-character hex key, or a NIP-05 identifier like name@example.com.' }
  if (pubkey === self) return { error: 'That is your own key.' }
  if (following.has(pubkey)) return { error: 'You already follow this key.' }
  return { pubkey }
}

/** Name, picture and NIP-05 from a kind 0, defensively. */
export function parseFollowProfile(event: Pick<NostrEvent, 'content' | 'created_at'>): FollowProfile {
  const content = parseProfileContent(event.content) ?? {}
  const text = (key: string) => typeof content[key] === 'string' && (content[key] as string).trim() ? (content[key] as string).trim() : undefined
  const picture = text('picture')
  return {
    name: text('display_name') ?? text('name'),
    picture: picture && /^https?:\/\//iu.test(picture) ? picture : undefined,
    nip05: text('nip05'),
    createdAt: event.created_at
  }
}

export function shortNpub(pubkey: string): string {
  const npub = npubEncode(pubkey)
  return `${npub.slice(0, 12)}…${npub.slice(-6)}`
}

/** The name to show: the profile's, then the petname, then a shortened npub (Porwr's order). */
export function displayName(follow: Follow, profile?: FollowProfile): string {
  return profile?.name ?? (follow.tag[3]?.trim() || shortNpub(follow.pubkey))
}

/** Splits keys into chunks for kind 0 lookups, so a large list does not make one huge filter. */
export function chunks<T>(items: T[], size: number): T[][] {
  const out: T[][] = []
  for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size))
  return out
}
