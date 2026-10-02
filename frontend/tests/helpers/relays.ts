// A fake RelayIO for the profile tests (#30): relays that hold events in memory, some of which can
// be made unreachable, and a record of what was published.
import type { NostrEvent } from 'nostr-tools/pure'
import type { Filter } from 'nostr-tools/filter'
import type { PublishResult, QueryResult, RelayIO } from '~/utils/relay-io'

export class FakeRelays implements RelayIO {
  events: NostrEvent[] = []
  published: NostrEvent[] = []
  down = new Set<string>()
  /** When set, every relay refuses to store. */
  refuse = false
  /** Events (by id) that only the named relays hold; every other event is on every relay. */
  only: Record<string, string[]> = {}
  /** The relays each publish went to. */
  publishedTo: string[][] = []
  queries = 0

  async query(relays: string[], filter: Filter): Promise<QueryResult> {
    this.queries++
    const reached = relays.filter(url => !this.down.has(url))
    const held = (id: string) => {
      const holders = Object.entries(this.only).filter(([, ids]) => ids.includes(id)).map(([url]) => url)
      return holders.length === 0 || holders.some(url => reached.includes(url))
    }
    const events = reached.length
      ? this.events.filter(e => (!filter.kinds || filter.kinds.includes(e.kind)) && (!filter.authors || filter.authors.includes(e.pubkey)) && held(e.id))
      : []
    return { events, reached, failed: relays.filter(url => this.down.has(url)) }
  }

  async publish(relays: string[], event: NostrEvent): Promise<PublishResult> {
    this.published.push(event)
    this.publishedTo.push(relays)
    if (this.refuse) return { accepted: [], failed: relays.map(url => ({ url, reason: 'blocked' })) }
    this.events.push(event)
    return { accepted: relays, failed: [] }
  }
}

let id = 0
export function profileEvent(pubkey: string, content: Record<string, unknown>, created_at: number): NostrEvent {
  return { id: String(++id).padStart(64, '0'), pubkey, kind: 0, created_at, tags: [], content: JSON.stringify(content), sig: '' }
}
