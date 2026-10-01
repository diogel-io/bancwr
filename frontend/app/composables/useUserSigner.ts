// The signed-in member's own signer, for signing their events after sign-in (#30): a profile
// update, an image upload. Never the bunker's key. Client-only.
import { verifyEvent, type NostrEvent } from 'nostr-tools/pure'
import { nip07Extension } from '~/composables/useNip07'
import type { NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

export type SignerMethod = 'nip07' | 'nip46'

const METHOD_KEY = 'bancwr-signer-method'

/** Sign-in records how the member signed in, in this tab only, as the NIP-46 connection is. */
export function rememberSignerMethod(method: SignerMethod) {
  if (import.meta.client) sessionStorage.setItem(METHOD_KEY, method)
}

export function forgetSignerMethod() {
  if (import.meta.client) sessionStorage.removeItem(METHOD_KEY)
}

export function signerMethod(): SignerMethod | undefined {
  if (!import.meta.client) return undefined
  const method = sessionStorage.getItem(METHOD_KEY)
  return method === 'nip07' || method === 'nip46' ? method : undefined
}

export class SignerMismatch extends Error {
  constructor() {
    super('Your signer signed with a different key from the one you are signed in with, so nothing was saved.')
  }
}

/**
 * Wraps `signer` so every event it returns is checked before anyone uses it: signed by `pubkey`,
 * and valid as received. A wrong key or a bad signature throws SignerMismatch.
 */
export function checkedSigner(signer: NostrSigner, pubkey: string): NostrSigner {
  return {
    async signEvent(template) {
      const event: SignedEvent = await signer.signEvent(template)
      // Verified as plain data: nostr-tools caches a verified flag on event objects, and a copy
      // spread from a cached object would pass whatever it says (README, "Nostr").
      const received = JSON.parse(JSON.stringify(event)) as NostrEvent
      if (received.pubkey !== pubkey || !verifyEvent(received)) throw new SignerMismatch()
      return received
    }
  }
}

export function useUserSigner() {
  const auth = useAuth()
  const nip46 = useNip46()

  /** The session's pubkey (hex), when signed in. */
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : undefined)

  /**
   * A signer holding the signed-in key, or undefined: the caller then asks the member to
   * reconnect (SignerConnect). NIP-46 resumes this tab's connection; NIP-07 is used only if the
   * extension now holds the same key.
   */
  async function signer(): Promise<NostrSigner | undefined> {
    const expected = pubkey.value
    if (!expected) return undefined
    const method = signerMethod()

    if (method === 'nip46') {
      const remote = nip46.resume()
      if (remote) return checkedSigner(remote, expected)
    }
    const extension = nip07Extension()
    if (extension && method !== 'nip46') {
      const key = await extension.getPublicKey().catch(() => undefined)
      if (key === expected) return checkedSigner(extension, expected)
    }
    return undefined
  }

  return { pubkey, signer }
}
