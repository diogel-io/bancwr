// The relay search for the bunker's own relays (#78): NIP-66 relay discovery events (kind 30166),
// read from the console's indexer relays (NUXT_PUBLIC_INDEXER_RELAYS, #62) with the same relay IO
// as the member pages. When no monitor answers, a short built-in list stands in, so the search is
// never empty. Client-only: it opens WebSockets from the browser.
import { DEFAULT_INDEXER_RELAYS, parseRelayList } from '~/utils/profile-relays'
import { DISCOVERY_LIMIT, FALLBACK_RELAYS, knownRelays, RELAY_DISCOVERY_KIND, type KnownRelay } from '~/utils/relay-discovery'
import { createRelayIO, type RelayIO } from '~/utils/relay-io'

export type DiscoveryState = 'idle' | 'searching' | 'found' | 'fallback'

export function useRelayDiscovery(deps: { io?: RelayIO } = {}) {
  const config = useRuntimeConfig().public
  const configured = parseRelayList(config.indexerRelays as string | string[] | undefined)
  const indexers = configured.length ? configured : DEFAULT_INDEXER_RELAYS

  let io = deps.io
  const relayIO = () => (io ??= createRelayIO())

  const state = ref<DiscoveryState>('idle')
  const relays = ref<KnownRelay[]>([])

  async function search() {
    state.value = 'searching'
    // A failed read is the same as no monitor answering: the fallback list.
    const found = await relayIO()
      .query(indexers, { kinds: [RELAY_DISCOVERY_KIND], limit: DISCOVERY_LIMIT })
      .then(result => knownRelays(result.events), () => [] as KnownRelay[])
    if (found.length) {
      relays.value = found
      state.value = 'found'
    } else {
      relays.value = [...FALLBACK_RELAYS]
      state.value = 'fallback'
    }
  }

  return { indexers, state, relays, search }
}
