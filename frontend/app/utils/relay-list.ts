// The NIP-65 relay list (kind 10002) (#33), read at nips commit 0046368a (2026-09-27): `r` tags
// with an optional `read` or `write` marker, none meaning both. Replaceable, like kind 3 (#32),
// so a publish replaces the whole list: changes are applied to the latest event, keeping every
// tag and the content they do not touch.
import type { NostrEvent } from 'nostr-tools/pure'
import { relayKey } from '~/utils/relay-url'

export interface RelayEntry {
  url: string
  read: boolean
  write: boolean
  /** The `r` tag as it was. */
  tag: string[]
}

export interface Markers {
  read: boolean
  write: boolean
}

/** Staged changes, keyed by relayKey(). */
export interface RelayChanges {
  /** New relays: the URL to write, and its markers. */
  adds: { url: string, read: boolean, write: boolean }[]
  removes: string[]
  /** New markers for relays already in the list. */
  markers: Record<string, Markers>
}

export function markersOf(tag: string[]): Markers {
  const marker = tag[2]
  return { read: marker !== 'write', write: marker !== 'read' }
}

/** The relays in a kind 10002, first occurrence per relay, each tag kept as it was. */
export function parseRelayEntries(event: Pick<NostrEvent, 'tags'> | undefined): RelayEntry[] {
  const seen = new Set<string>()
  const entries: RelayEntry[] = []
  for (const tag of event?.tags ?? []) {
    if (tag[0] !== 'r' || !tag[1] || !/^wss?:\/\//iu.test(tag[1])) continue
    const key = relayKey(tag[1])
    if (seen.has(key)) continue
    seen.add(key)
    entries.push({ url: tag[1], ...markersOf(tag), tag })
  }
  return entries
}

/** The `r` tag for `url` with `markers`. */
export function relayTag(url: string, markers: Markers): string[] {
  if (markers.read && markers.write) return ['r', url]
  return ['r', url, markers.read ? 'read' : 'write']
}

/**
 * The tags and content to publish: the latest list with removed relays' `r` tags dropped, only
 * the tags whose markers changed rewritten, and added relays appended. Every other tag keeps its
 * place, and the content is kept.
 */
export function applyRelayChanges(latest: Pick<NostrEvent, 'tags' | 'content'> | undefined, changes: RelayChanges): { tags: string[][], content: string } {
  const removed = new Set(changes.removes)
  const done = new Set<string>()
  const tags: string[][] = []
  for (const tag of latest?.tags ?? []) {
    if (tag[0] !== 'r' || !tag[1]) {
      tags.push(tag)
      continue
    }
    const key = relayKey(tag[1])
    if (removed.has(key)) continue
    const markers = changes.markers[key]
    if (markers && !done.has(key)) {
      tags.push(relayTag(tag[1], markers))
      done.add(key)
    } else {
      tags.push(tag)
    }
  }
  const present = new Set(parseRelayEntries({ tags }).map(e => relayKey(e.url)))
  for (const add of changes.adds) {
    const key = relayKey(add.url)
    if (present.has(key) || removed.has(key)) continue
    tags.push(relayTag(add.url, add))
    present.add(key)
  }
  return { tags, content: latest?.content ?? '' }
}

/** Relays by key with their markers, for comparing two lists. */
function relayMap(tags: string[][]): Map<string, Markers> {
  return new Map(parseRelayEntries({ tags }).map(e => [relayKey(e.url), { read: e.read, write: e.write }]))
}

/**
 * Whether `next` differs from `latest` by exactly the staged changes: the relays are the latest's,
 * minus removals, plus adds, with the staged markers and every other marker unchanged; no other
 * tag and not the content changed. A last check before signing (#32's pattern).
 */
export function onlyStagedRelayChanges(latest: Pick<NostrEvent, 'tags' | 'content'> | undefined, next: { tags: string[][], content: string }, changes: RelayChanges): boolean {
  const expected = relayMap(latest?.tags ?? [])
  for (const key of changes.removes) expected.delete(key)
  for (const [key, markers] of Object.entries(changes.markers)) if (expected.has(key)) expected.set(key, markers)
  for (const add of changes.adds) {
    const key = relayKey(add.url)
    if (!expected.has(key) && !changes.removes.includes(key)) expected.set(key, { read: add.read, write: add.write })
  }
  const actual = relayMap(next.tags)
  if (actual.size !== expected.size) return false
  for (const [key, markers] of expected) {
    const got = actual.get(key)
    if (!got || got.read !== markers.read || got.write !== markers.write) return false
  }
  const others = (tags: string[][]) => JSON.stringify(tags.filter(t => t[0] !== 'r'))
  return others(latest?.tags ?? []) === others(next.tags) && (latest?.content ?? '') === next.content
}
