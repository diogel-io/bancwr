// The signed-in member's follow list (kind 3) (#32): read from their relays, changes staged, and
// published together, signed by their own signer. Kind 3 is replaceable, so save() is guarded so
// that nothing the member did not remove can be lost. Client-only: call load() from onMounted.
import type { NostrEvent } from 'nostr-tools/pure'
import { applyChanges, chunks, onlyStagedChanges, parseFollowProfile, parseFollows, type FollowProfile } from '~/utils/follows'
import { newestEvent } from '~/utils/profile'
import type { PublishResult, RelayIO } from '~/utils/relay-io'
import type { NostrSigner } from '~/utils/nostr-sign-in'

export type FollowsState = 'idle' | 'loading' | 'loaded' | 'failed'

export class FollowsError extends Error {}

export const NO_RELAY_ON_SAVE = 'Couldn\'t read your follow list from any relay, so saving now could drop follows. Check your connection and try again.'
export const INCOMPLETE_READ = 'Couldn\'t read your full follow list: the relays that had it did not answer this time, so saving now could drop follows. Try again.'

/** Kind 0 lookups per request, so a list of thousands does not become one huge filter. */
export const PROFILE_CHUNK = 250

export function useFollows(deps: { io?: RelayIO } = {}) {
  const member = useMemberRelays(deps)
  const { relays, searched } = member
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

  const state = ref<FollowsState>('idle')
  /** The newest kind 3 as last loaded or saved; undefined when none was found. */
  const loaded = shallowRef<NostrEvent>()
  const follows = computed(() => parseFollows(loaded.value))
  const following = computed(() => new Set(follows.value.map(f => f.pubkey)))
  const exists = computed(() => !!loaded.value)
  const pendingAdds = ref<string[]>([])
  const pendingRemoves = ref<string[]>([])
  const dirty = computed(() => pendingAdds.value.length > 0 || pendingRemoves.value.length > 0)
  const profiles = ref<Record<string, FollowProfile>>({})
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

  /** The newest kind 3 across the member's relays, and how many relays answered. */
  async function latest(): Promise<{ event?: NostrEvent, reached: number }> {
    const result = await member.query({ kinds: [3], authors: [pubkey.value] })
    return { event: newestEvent(result.events.filter(e => e.kind === 3 && e.pubkey === pubkey.value)), reached: result.reached.length }
  }

  async function readList() {
    const { event, reached } = await latest()
    if (reached === 0) {
      state.value = 'failed'
      return
    }
    loaded.value = event
    pendingAdds.value = []
    pendingRemoves.value = []
    state.value = 'loaded'
    void loadProfiles(follows.value.map(f => f.pubkey))
  }

  /** Names and pictures for `keys`, in chunks, filling in as they arrive. Best effort. */
  async function loadProfiles(keys: string[]) {
    const wanted = keys.filter(key => !profiles.value[key])
    await Promise.all(chunks(wanted, PROFILE_CHUNK).map(async (authors) => {
      try {
        const result = await member.lookup({ kinds: [0], authors })
        const found: Record<string, FollowProfile> = {}
        for (const event of result.events) {
          if (event.kind !== 0 || !authors.includes(event.pubkey)) continue
          const profile = parseFollowProfile(event)
          const known = found[event.pubkey]
          if (!known || profile.createdAt > known.createdAt) found[event.pubkey] = profile
        }
        profiles.value = { ...profiles.value, ...found }
      } catch {
        // Names are a convenience: the list still shows npubs.
      }
    }))
  }

  /** Stages following `key`, or undoes a staged unfollow of it. */
  function stageAdd(key: string) {
    if (pendingRemoves.value.includes(key)) {
      pendingRemoves.value = pendingRemoves.value.filter(k => k !== key)
    } else if (!following.value.has(key) && !pendingAdds.value.includes(key)) {
      pendingAdds.value = [...pendingAdds.value, key]
      void loadProfiles([key])
    }
  }

  /** Stages unfollowing `key`, or undoes a staged follow of it. */
  function stageRemove(key: string) {
    if (pendingAdds.value.includes(key)) {
      pendingAdds.value = pendingAdds.value.filter(k => k !== key)
    } else if (following.value.has(key) && !pendingRemoves.value.includes(key)) {
      pendingRemoves.value = [...pendingRemoves.value, key]
    }
  }

  function discard() {
    pendingAdds.value = []
    pendingRemoves.value = []
  }

  /**
   * Publishes the staged changes. Guarded, because a kind 3 replaces the whole list:
   * 1. the newest list is read again now; no relay answering, or an older list than the one
   *    loaded (an incomplete read), refuses;
   * 2. the changes are applied to that list, so follows made elsewhere since loading are kept;
   * 3. the result must differ from it by exactly the staged changes, or nothing is signed.
   */
  async function save(signer: NostrSigner): Promise<PublishResult> {
    const { event: current, reached } = await latest()
    if (reached === 0) throw new FollowsError(NO_RELAY_ON_SAVE)
    if (loaded.value && (!current || current.created_at < loaded.value.created_at)) {
      throw new FollowsError(INCOMPLETE_READ)
    }

    const adds = [...pendingAdds.value]
    const removes = [...pendingRemoves.value]
    const next = applyChanges(current, adds, removes)
    if (!onlyStagedChanges(current, next, adds, removes)) {
      throw new FollowsError('The update would have changed more than you asked, so nothing was published. Reload and try again.')
    }

    const now = Math.floor(Date.now() / 1000)
    // A fresh event, never an edited copy of a signed one (README); newer than the current one.
    const event = await signer.signEvent({
      kind: 3,
      created_at: Math.max(now, (current?.created_at ?? 0) + 1),
      tags: next.tags,
      content: next.content
    })

    const result = await member.publish(event as NostrEvent)
    lastPublish.value = result
    if (result.accepted.length === 0) {
      throw new FollowsError('No relay accepted the update, so your follow list has not changed. Try again.')
    }
    loaded.value = event as NostrEvent
    pendingAdds.value = []
    pendingRemoves.value = []
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
    state, follows, following, exists, pendingAdds, pendingRemoves, dirty, profiles, lastPublish, pubkey, relays, searched,
    load, save, stageAdd, stageRemove, discard, searchRelay, loadProfiles
  }
}
