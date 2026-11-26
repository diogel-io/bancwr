// Whose account the Profile, Follows and Relays pages edit (#150). On /profile, /follows and
// /relays: the signed-in member's own, signed by their own signer, as before (#30, #32, #33). On
// /identities/:pubkey/profile, /follows and /relays, which only administrators may open: one of the
// bunker's identities, signed by the bunker as it (utils/identity-signer.ts).
//
// The key is read once, when the page is set up: each route gets its own page instance.
import { decode } from 'nostr-tools/nip19'
import type { ComputedRef, Ref } from 'vue'
import type { BunkerIdentity } from '#shared/types/bunker'
import { identitySigner } from '~/utils/identity-signer'
import type { NostrSigner } from '~/utils/nostr-sign-in'

/** Hex for a hex or npub key, or undefined when it is neither. */
export function identityKey(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  if (/^[0-9a-f]{64}$/u.test(value)) return value
  try {
    const decoded = decode(value)
    return decoded.type === 'npub' ? decoded.data : undefined
  } catch {
    return undefined
  }
}

export interface EditTarget {
  /**
   * The key for the editors' `pubkey`: undefined for the member's own account; the identity's hex
   * key, or '' when the route's is not a key, so nothing is read and nothing falls back to the
   * member's own.
   */
  pubkey: string | undefined
  /** Whether the page edits one of the bunker's identities. */
  editingIdentity: boolean
  /** That identity, once the list has loaded and it is in it. */
  identity: ComputedRef<BunkerIdentity | undefined>
  /** The list has loaded and holds no such identity, or the key is not a key. */
  missing: ComputedRef<boolean>
  withSigner: (work: (signer: NostrSigner) => Promise<void>) => Promise<void>
  /** The member's own signer's reconnect dialog: never opens for an identity. */
  reconnecting: Ref<boolean>
  reconnected: ReturnType<typeof useSignerPrompt>['reconnected']
}

export function useEditTarget(): EditTarget {
  const route = useRoute()
  const prompt = useSignerPrompt()
  if (!('pubkey' in route.params)) {
    return {
      pubkey: undefined,
      editingIdentity: false,
      identity: computed(() => undefined),
      missing: computed(() => false),
      ...prompt
    }
  }

  const pubkey = identityKey(route.params.pubkey) ?? ''
  const { identities, list } = useIdentities()
  const identity = computed(() => pubkey ? list.value.find(i => i.pubkey === pubkey) : undefined)
  const missing = computed(() => !pubkey || (identities.status.value === 'success' && !identity.value))
  return {
    pubkey,
    editingIdentity: true,
    identity,
    missing,
    async withSigner(work) {
      if (!pubkey) return
      await work(identitySigner(pubkey))
    },
    reconnecting: prompt.reconnecting,
    reconnected: prompt.reconnected
  }
}
