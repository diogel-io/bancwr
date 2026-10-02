// Running work that needs the signed-in member's signer (#30, #32): their signer if one is
// available, otherwise after they reconnect it in the page's "Connect your signer" dialog
// (SignerConnect). Closing the dialog without connecting cancels the work with SignerCancelled.
import { checkedSigner, rememberSignerMethod, type SignerMethod } from '~/composables/useUserSigner'
import type { NostrSigner } from '~/utils/nostr-sign-in'

/** The member closed the reconnect dialog without connecting. */
export class SignerCancelled extends Error {}

export function useSignerPrompt() {
  const userSigner = useUserSigner()
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

  /** Bound to the dialog's open state. */
  const reconnecting = ref(false)
  let pending: { work: (signer: NostrSigner) => Promise<void>, resolve: () => void, reject: (error: Error) => void } | undefined

  async function withSigner(work: (signer: NostrSigner) => Promise<void>): Promise<void> {
    const signer = await userSigner.signer()
    if (signer) return work(signer)
    pending?.reject(new SignerCancelled())
    return new Promise((resolve, reject) => {
      pending = { work, resolve, reject }
      reconnecting.value = true
    })
  }

  /** SignerConnect's `use`: the reconnected signer runs the waiting work. */
  async function reconnected(signer: NostrSigner, method: SignerMethod): Promise<string | undefined> {
    rememberSignerMethod(method)
    const waiting = pending
    pending = undefined
    reconnecting.value = false
    if (!waiting) return undefined
    try {
      await waiting.work(checkedSigner(signer, pubkey.value))
      waiting.resolve()
    } catch (failure) {
      waiting.reject(failure instanceof Error ? failure : new Error(String(failure)))
    }
    return undefined
  }

  watch(reconnecting, (open) => {
    if (open || !pending) return
    const waiting = pending
    pending = undefined
    waiting.reject(new SignerCancelled())
  })

  return { reconnecting, withSigner, reconnected }
}
