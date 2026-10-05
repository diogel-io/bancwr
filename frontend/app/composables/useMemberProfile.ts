// Another vault member's kind 0 profile (#77), read-only, for the team member page: read from
// their relays exactly as the profile page reads the signed-in member's own (#30, #62), through the
// same relay lookup (useMemberRelays), and never published. Client-only: call load() from onMounted.
import { emptyForm, formFromContent, newestEvent, parseProfileContent, type ProfileForm } from '~/utils/profile'
import type { RelayIO } from '~/utils/relay-io'
import type { LoadState } from '~/composables/useProfile'

export function useMemberProfile(deps: { io?: RelayIO } = {}) {
  const member = useMemberRelays(deps)
  const state = ref<LoadState>('idle')
  const form = ref<ProfileForm>(emptyForm())
  /** Whether any relay holds a profile for this key. */
  const exists = ref(false)
  let key = ''

  async function read() {
    const result = await member.query({ kinds: [0], authors: [key] })
    if (result.reached.length === 0) {
      state.value = 'failed'
      return
    }
    const event = newestEvent(result.events.filter(e => e.kind === 0 && e.pubkey === key))
    exists.value = !!event
    form.value = formFromContent(event ? parseProfileContent(event.content) : null)
    state.value = 'loaded'
  }

  /** Reads the newest profile of `pubkey` (hex) from their relays. */
  async function load(pubkey: string) {
    key = pubkey
    if (!key) return
    state.value = 'loading'
    try {
      await member.resolve(key)
      await read()
    } catch {
      state.value = 'failed'
    }
  }

  return { state, form, exists, searched: member.searched, load }
}
