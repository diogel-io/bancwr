// A NIP-07 browser extension (window.nostr), if there is one (#11). Client-only.
import type { NostrSigner } from '~/utils/nostr-sign-in'

/**
 * `undefined` until checked, then whether an extension is present. Extensions inject
 * window.nostr at document_end (NIP-07), which can land just after the app mounts, so this waits
 * up to about a second before saying there is none.
 */
export function useNip07() {
  const available = ref<boolean | undefined>(undefined)

  onMounted(async () => {
    for (let i = 0; i < 10 && !extension(); i++) {
      await new Promise(resolve => setTimeout(resolve, 100))
    }
    available.value = !!extension()
  })

  return { available, signer: (): NostrSigner | undefined => extension() }
}

function extension(): NostrSigner | undefined {
  return (window as unknown as { nostr?: NostrSigner }).nostr
}
