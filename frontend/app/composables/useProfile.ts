// The signed-in member's kind 0 profile (#30): reading it from their relays and publishing an
// update signed by their own signer. Client-only: call load() from onMounted.
import type { NostrEvent } from 'nostr-tools/pure'
import { emptyForm, formFromContent, mergeProfile, newestEvent, parseProfileContent, type ProfileForm } from '~/utils/profile'
import type { PublishResult, RelayIO } from '~/utils/relay-io'
import type { NostrSigner } from '~/utils/nostr-sign-in'

export type LoadState = 'idle' | 'loading' | 'loaded' | 'failed'

export class ProfileError extends Error {}

export const PROFILE_NO_RELAY_ON_SAVE = 'Couldn\'t read your current profile from any relay, so saving now could lose fields set in other apps. Check your connection and try again.'

function copy(form: ProfileForm): ProfileForm {
  return { ...form, birthday: { ...form.birthday } }
}

export function useProfile(deps: { io?: RelayIO } = {}) {
  const member = useMemberRelays(deps)
  const { relays, searched, blossomServer } = member
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

  const state = ref<LoadState>('idle')
  const form = ref<ProfileForm>(emptyForm())
  /** The form as last loaded or saved, to tell whether there are unsaved changes. */
  const saved = ref<ProfileForm>(emptyForm())
  /** Whether any relay holds a profile for this key. */
  const exists = ref(false)
  const lastPublish = ref<PublishResult>()

  const dirty = computed(() => JSON.stringify(form.value) !== JSON.stringify(saved.value))

  /** The newest kind 0 across the member's relays, and how many relays answered. */
  async function latest(): Promise<{ event?: NostrEvent, reached: number }> {
    const result = await member.query({ kinds: [0], authors: [pubkey.value] })
    return { event: newestEvent(result.events.filter(e => e.pubkey === pubkey.value)), reached: result.reached.length }
  }

  async function load() {
    if (!pubkey.value) return
    state.value = 'loading'
    try {
      await member.resolve(pubkey.value)
      await readProfile()
    } catch {
      state.value = 'failed'
    }
  }

  async function readProfile() {
    const { event, reached } = await latest()
    if (reached === 0) {
      state.value = 'failed'
      return
    }
    exists.value = !!event
    form.value = formFromContent(event ? parseProfileContent(event.content) : null)
    saved.value = copy(form.value)
    state.value = 'loaded'
  }

  /**
   * Adds a relay to search, for this visit, and reads the profile again (#62). Returns a message
   * when `url` is not a relay address. Unsaved edits are replaced by what is found.
   */
  async function searchRelay(url: string): Promise<string | undefined> {
    const problem = member.addRelay(url)
    if (problem) return problem
    state.value = 'loading'
    try {
      await readProfile()
    } catch {
      state.value = 'failed'
    }
    return undefined
  }

  /**
   * Publishes the form, merged into the newest profile read again now: every field the form does
   * not edit is kept (#30). Refuses when no relay answers, rather than risk dropping fields.
   */
  async function save(signer: NostrSigner): Promise<PublishResult> {
    const { event: current, reached } = await latest()
    if (reached === 0) throw new ProfileError(PROFILE_NO_RELAY_ON_SAVE)

    const content = mergeProfile(current ? parseProfileContent(current.content) : null, form.value)
    const now = Math.floor(Date.now() / 1000)
    // A fresh event, never an edited copy of a signed one (README); newer than the current one,
    // or relays would keep that instead.
    const event = await signer.signEvent({
      kind: 0,
      created_at: Math.max(now, (current?.created_at ?? 0) + 1),
      tags: [],
      content: JSON.stringify(content)
    })

    const result = await member.publish(event as NostrEvent)
    lastPublish.value = result
    if (result.accepted.length === 0) {
      throw new ProfileError('No relay accepted the update, so your profile has not changed. Try again.')
    }
    exists.value = true
    saved.value = copy(form.value)
    return result
  }

  return { state, form, saved, dirty, exists, relays, searched, blossomServer, lastPublish, pubkey, load, save, searchRelay }
}
