// The signed-in key's name and picture (#72): one lookup per key, shared by the sidebar footer's
// avatar and the "Signed in as" line (#70), and replaced straight away when the profile page saves.
// Best effort and bounded (utils/key-profile.ts); browser-only, so call load() from onMounted.
import { parseFollowProfile } from '~/utils/follows'
import { lookupKeyProfile, lookupRelays, queryRelays } from '~/utils/key-profile'

export interface SignedInProfile {
  name?: string
  /** Only ever an http(s) URL: parseFollowProfile drops anything else. */
  picture?: string
}

interface SignedInProfileState {
  /** The key this state is for. A lookup or save for any other key is ignored. */
  pubkey: string
  status: 'idle' | 'loading' | 'done'
  profile?: SignedInProfile
}

export function useSignedInProfile() {
  const state = useState<SignedInProfileState>('signed-in-profile', () => ({ pubkey: '', status: 'idle' }))
  const auth = useAuth()
  const config = useRuntimeConfig().public
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

  /** Looks the signed-in key up, once per key. A later call for the same key does nothing. */
  async function load(): Promise<void> {
    const key = pubkey.value
    if (!key || (state.value.pubkey === key && state.value.status !== 'idle')) return

    state.value = { pubkey: key, status: 'loading' }
    const found = await lookupKeyProfile(key, lookupRelays(config.profileRelays, config.indexerRelays), queryRelays)

    // Signed out, or in as another key, while it looked; or the profile page saved meanwhile,
    // which is newer than anything a relay returned.
    if (state.value.pubkey !== key || state.value.status !== 'loading') return
    state.value = { pubkey: key, status: 'done', profile: found && { name: found.name, picture: found.picture } }
  }

  /** What the profile page just published, so the footer shows it without a reload. */
  function setFromContent(content: string): void {
    if (!pubkey.value) return
    const { name, picture } = parseFollowProfile({ content, created_at: 0 })
    state.value = { pubkey: pubkey.value, status: 'done', profile: { name, picture } }
  }

  const current = computed(() => state.value.pubkey === pubkey.value ? state.value : undefined)

  return {
    pubkey,
    profile: computed(() => current.value?.profile),
    loading: computed(() => current.value?.status === 'loading'),
    load,
    setFromContent
  }
}
