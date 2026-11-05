// A NIP-07 browser extension (window.nostr), if there is one (#11). Client-only.
import type { NostrSigner } from '~/utils/nostr-sign-in'

/** NIP-07's window.nostr: signEvent, and getPublicKey (hex), which the page asks first. */
export interface Nip07Extension extends NostrSigner {
  getPublicKey(): Promise<string>
}

/**
 * `undefined` until checked, then whether an extension is present. Extensions inject
 * window.nostr at document_end (NIP-07), which can land just after the app mounts, so this waits
 * up to about a second before saying there is none.
 */
export function useNip07() {
  const available = ref<boolean | undefined>(undefined)

  // Stop polling on unmount, so a pending check never runs after the page (or a test's DOM) is gone.
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let wake: (() => void) | undefined
  onUnmounted(() => {
    stopped = true
    clearTimeout(timer)
    wake?.()
  })

  onMounted(async () => {
    for (let i = 0; i < 10 && !stopped && !extension(); i++) {
      await new Promise<void>((resolve) => {
        wake = resolve
        timer = setTimeout(resolve, 100)
      })
    }
    if (!stopped) available.value = !!extension()
  })

  return { available, signer: (): Nip07Extension | undefined => extension() }
}

/** The extension, if one is present now. */
export function nip07Extension(): Nip07Extension | undefined {
  return extension()
}

/** window.nostr, or undefined when there is no window (server side, or after a test DOM is torn down). */
function extension(): Nip07Extension | undefined {
  if (typeof window === 'undefined') return undefined
  return (window as unknown as { nostr?: Nip07Extension }).nostr
}
