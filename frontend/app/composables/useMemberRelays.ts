// Where the signed-in member's own events are read from and published to (#30, #62), shared by the
// profile (#30) and follow list (#32) pages: their NIP-65 write relays, then the deployment's
// defaults, then indexer relays, then any relay they asked to search on this visit. Their relay
// list and Blossom servers are looked up on the defaults and the indexers. Client-only.
import type { NostrEvent } from 'nostr-tools/pure'
import type { Filter } from 'nostr-tools/filter'
import { newestEvent } from '~/utils/profile'
import { blossomServerFrom, DEFAULT_BLOSSOM_SERVER, DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS, parseRelayList, parseRelayUrl, profileRelays } from '~/utils/profile-relays'
import { createRelayIO, type QueryResult, type RelayIO } from '~/utils/relay-io'

export interface Searched {
  reached: string[]
  failed: string[]
}

export function useMemberRelays(deps: { io?: RelayIO } = {}) {
  const config = useRuntimeConfig().public
  const configured = parseRelayList(config.profileRelays as string | string[] | undefined)
  const defaults = configured.length ? configured : DEFAULT_PROFILE_RELAYS
  const configuredIndexers = parseRelayList(config.indexerRelays as string | string[] | undefined)
  const indexers = configuredIndexers.length ? configuredIndexers : DEFAULT_INDEXER_RELAYS
  /** Where the member's relay list and Blossom servers are looked up (#62). */
  const discovery = [...new Set([...defaults, ...indexers])]
  const defaultBlossom = String(config.blossomServer || DEFAULT_BLOSSOM_SERVER).replace(/\/+$/u, '')

  let io = deps.io
  const relayIO = () => (io ??= createRelayIO())

  const relays = ref<string[]>(discovery)
  /** Relays the member asked to search on this visit (#62); not stored. */
  const extra = ref<string[]>([])
  /** The relays the last read asked, by whether they answered. */
  const searched = ref<Searched>({ reached: [], failed: [] })
  const blossomServer = ref(defaultBlossom)
  let list: NostrEvent | undefined

  function update() {
    relays.value = profileRelays(defaults, list, [...indexers, ...extra.value])
  }

  /**
   * The member's relay list (kind 10002) and Blossom servers (kind 10063), looked up on the
   * defaults and the indexers: a list held only by an indexer was missed before #62.
   */
  async function resolve(pubkey: string) {
    const lists = await relayIO().query(discovery, { kinds: [10002, 10063], authors: [pubkey] })
    list = newestEvent(lists.events.filter(e => e.kind === 10002 && e.pubkey === pubkey))
    blossomServer.value = blossomServerFrom(newestEvent(lists.events.filter(e => e.kind === 10063 && e.pubkey === pubkey))) ?? defaultBlossom
    update()
  }

  /** Reads `filter` from the member's relays, recording which answered. */
  async function query(filter: Filter): Promise<QueryResult> {
    const result = await relayIO().query(relays.value, filter)
    searched.value = { reached: result.reached, failed: result.failed }
    return result
  }

  /** Reads `filter` from the member's relays without touching `searched` (lookups, not the list). */
  function lookup(filter: Filter): Promise<QueryResult> {
    return relayIO().query(relays.value, filter)
  }

  function publish(event: NostrEvent) {
    return relayIO().publish(relays.value, event)
  }

  /** Publishes to `urls` rather than the member's relays: a relay list goes wider (#33). */
  function publishTo(urls: string[], event: NostrEvent) {
    return relayIO().publish(urls, event)
  }

  /** Adopts a relay list just published (#33), so reads and publishes use it from now on. */
  function setList(event: NostrEvent | undefined) {
    list = event
    update()
  }

  /** Adds a relay to search on this visit; returns a message when `url` is not a relay address. */
  function addRelay(url: string): string | undefined {
    const relay = parseRelayUrl(url)
    if (!relay) return 'Enter a relay address starting with wss://'
    if (!extra.value.includes(relay)) extra.value = [...extra.value, relay]
    update()
    return undefined
  }

  return { relays, searched, blossomServer, defaults, indexers, resolve, query, lookup, publish, publishTo, setList, addRelay }
}
