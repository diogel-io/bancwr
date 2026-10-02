// The signed-in member's NIP-65 relay list (kind 10002) (#33): read from their relays, changes
// staged, and published together, signed by their own signer. Replaceable, like the follow list
// (#32), so save() carries the same guards. Client-only: call load() from onMounted.
import type { NostrEvent } from 'nostr-tools/pure'
import { newestEvent } from '~/utils/profile'
import { applyRelayChanges, onlyStagedRelayChanges, parseRelayEntries, type Markers, type RelayChanges } from '~/utils/relay-list'
import { relayKey } from '~/utils/relay-url'
import type { PublishResult, RelayIO } from '~/utils/relay-io'
import type { NostrSigner } from '~/utils/nostr-sign-in'

export type RelayListState = 'idle' | 'loading' | 'loaded' | 'failed'

export class RelayListError extends Error {}

export const NO_RELAY_ON_SAVE = 'Couldn\'t read your relay list from any relay, so saving now could drop relays. Check your connection and try again.'
export const INCOMPLETE_READ = 'Couldn\'t read your full relay list: the relays that had it did not answer this time, so saving now could drop relays. Try again.'

export function useRelayList(deps: { io?: RelayIO } = {}) {
  const member = useMemberRelays(deps)
  const { relays, searched, defaults, indexers } = member
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

  const state = ref<RelayListState>('idle')
  /** The newest kind 10002 as last loaded or saved; undefined when none was found. */
  const loaded = shallowRef<NostrEvent>()
  const entries = computed(() => parseRelayEntries(loaded.value))
  const exists = computed(() => !!loaded.value)
  const adds = ref<RelayChanges['adds']>([])
  const removes = ref<string[]>([])
  const markers = ref<Record<string, Markers>>({})
  const dirty = computed(() => adds.value.length > 0 || removes.value.length > 0 || Object.keys(markers.value).length > 0)
  /** Relays that answered or did not when the list was read, by relayKey(). */
  const answered = ref<Set<string>>(new Set())
  const unanswered = ref<Set<string>>(new Set())
  const lastPublish = ref<PublishResult>()

  async function load() {
    if (!pubkey.value) return
    state.value = 'loading'
    try {
      await member.resolve(pubkey.value)
      await readList()
    } catch {
      state.value = 'failed'
    }
  }

  /** The newest kind 10002 across the member's relays, and how many relays answered. */
  async function latest(): Promise<{ event?: NostrEvent, reached: number }> {
    const result = await member.query({ kinds: [10002], authors: [pubkey.value] })
    answered.value = new Set(result.reached.map(relayKey))
    unanswered.value = new Set(result.failed.map(relayKey))
    return { event: newestEvent(result.events.filter(e => e.kind === 10002 && e.pubkey === pubkey.value)), reached: result.reached.length }
  }

  async function readList() {
    const { event, reached } = await latest()
    if (reached === 0) {
      state.value = 'failed'
      return
    }
    loaded.value = event
    discard()
    state.value = 'loaded'
  }

  const listed = (key: string) => entries.value.some(e => relayKey(e.url) === key)

  /** Stages a new relay, or undoes a staged removal of it. `url` is already normalised. */
  function stageAdd(url: string, read = true, write = true) {
    const key = relayKey(url)
    if (removes.value.includes(key)) {
      removes.value = removes.value.filter(k => k !== key)
    } else if (!listed(key) && !adds.value.some(a => relayKey(a.url) === key)) {
      adds.value = [...adds.value, { url, read, write }]
    }
  }

  /** Stages removing a relay, or drops a staged add of it. */
  function stageRemove(url: string) {
    const key = relayKey(url)
    if (adds.value.some(a => relayKey(a.url) === key)) {
      adds.value = adds.value.filter(a => relayKey(a.url) !== key)
    } else if (listed(key) && !removes.value.includes(key)) {
      removes.value = [...removes.value, key]
      const { [key]: _, ...rest } = markers.value
      markers.value = rest
    }
  }

  /**
   * Stages new markers for a relay. Neither read nor write is a removal: a relay with no use is
   * not a relay in the list. Setting a listed relay back to its markers clears the change.
   */
  function stageMarkers(url: string, next: Markers) {
    const key = relayKey(url)
    if (!next.read && !next.write) {
      stageRemove(url)
      return
    }
    const added = adds.value.find(a => relayKey(a.url) === key)
    if (added) {
      adds.value = adds.value.map(a => relayKey(a.url) === key ? { ...a, ...next } : a)
      return
    }
    const entry = entries.value.find(e => relayKey(e.url) === key)
    if (!entry) return
    const { [key]: _, ...rest } = markers.value
    markers.value = entry.read === next.read && entry.write === next.write ? rest : { ...rest, [key]: next }
  }

  function discard() {
    adds.value = []
    removes.value = []
    markers.value = {}
  }

  /** The list as it will be after saving, for display and guidance. */
  const preview = computed(() => parseRelayEntries(applyRelayChanges(loaded.value, changes())))

  function changes(): RelayChanges {
    return { adds: [...adds.value], removes: [...removes.value], markers: { ...markers.value } }
  }

  /**
   * Publishes the staged changes, with #32's guards: the newest list read again now, refusing when
   * no relay answers or it is older than the list loaded; the changes applied to it; and the
   * result checked to differ by exactly the staged changes before anything is signed. Published
   * to the old list's relays, the new list's, the defaults and the indexers (NIP-65: spread it).
   */
  async function save(signer: NostrSigner): Promise<PublishResult> {
    const { event: current, reached } = await latest()
    if (reached === 0) throw new RelayListError(NO_RELAY_ON_SAVE)
    if (loaded.value && (!current || current.created_at < loaded.value.created_at)) {
      throw new RelayListError(INCOMPLETE_READ)
    }

    const staged = changes()
    const next = applyRelayChanges(current, staged)
    if (!onlyStagedRelayChanges(current, next, staged)) {
      throw new RelayListError('The update would have changed more than you asked, so nothing was published. Reload and try again.')
    }

    const now = Math.floor(Date.now() / 1000)
    // A fresh event, never an edited copy of a signed one (README); newer than the current one.
    const event = await signer.signEvent({
      kind: 10002,
      created_at: Math.max(now, (current?.created_at ?? 0) + 1),
      tags: next.tags,
      content: next.content
    }) as NostrEvent

    const targets = [...new Set([
      ...parseRelayEntries(current).map(e => e.url),
      ...parseRelayEntries(next).map(e => e.url),
      ...defaults,
      ...indexers
    ].map(relayKey))]
    const result = await member.publishTo(targets, event)
    lastPublish.value = result
    if (result.accepted.length === 0) {
      throw new RelayListError('No relay accepted the update, so your relay list has not changed. Try again.')
    }
    loaded.value = event
    member.setList(event)
    discard()
    return result
  }

  /** Adds a relay to search, for this visit, and reads the list again (#62's not-found flow). */
  async function searchRelay(url: string): Promise<string | undefined> {
    const problem = member.addRelay(url)
    if (problem) return problem
    state.value = 'loading'
    try {
      await readList()
    } catch {
      state.value = 'failed'
    }
    return undefined
  }

  return {
    state, entries, preview, exists, adds, removes, markers, dirty, answered, unanswered, lastPublish, pubkey, relays, searched, defaults, indexers,
    load, save, stageAdd, stageRemove, stageMarkers, discard, searchRelay
  }
}
