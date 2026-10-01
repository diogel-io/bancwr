// A NIP-46 remote signer, reached through relays (#11). Client-only: relay I/O never runs during
// SSR (see "Nostr (nostr-tools)" in the README).
//
// The client keypair is disposable and is not the user's key (NIP-46). It lives in this tab's
// sessionStorage, so later signing (#30) can reuse the connection, and is deleted on sign-out,
// after a courtesy `logout` to the signer.
import { BunkerSigner } from 'nostr-tools/nip46'
import { generateSecretKey } from 'nostr-tools/pure'
import { bytesToHex, hexToBytes } from 'nostr-tools/utils'
import type { NostrSigner } from '~/utils/nostr-sign-in'
import { parseBunkerUri, type BunkerPointer } from '~/utils/nip46'
/** A NIP-46 signer as the app uses it: signing, and which key it signs with. */
export interface RemoteSigner extends NostrSigner {
  getPublicKey(): Promise<string>
}

const STORAGE_KEY = 'bancwr-nip46'
/** No answer to `connect` in this long: wrong relay, or the signer is offline. */
export const CONNECT_TIMEOUT_MS = 30_000
/** Inside the 120 s window a sign-in event is valid for (ADR rule 5), allowing for the round trip. */
export const SIGN_TIMEOUT_MS = 110_000

export type Nip46Phase = 'idle' | 'connecting' | 'signing' | 'awaiting-approval'

export class Nip46Cancelled extends Error {
  constructor() {
    super('Cancelled')
  }
}

export class Nip46Timeout extends Error {}

let live: BunkerSigner | undefined
let cancelLive: (() => void) | undefined

export function useNip46() {
  const phase = useState<Nip46Phase>('nip46-phase', () => 'idle')
  /** A URL the signer asked the user to open to approve (NIP-46 auth_url); never opened for them. */
  const approvalUrl = useState<string | undefined>('nip46-approval-url', () => undefined)

  /** Connects to the signer in `uri` and returns a NostrSigner that signs through it. */
  async function connect(uri: string): Promise<RemoteSigner> {
    const pointer = parseBunkerUri(uri)
    if ('error' in pointer) throw new Error(pointer.error)

    await close()
    const clientKey = generateSecretKey()
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify({ clientKey: bytesToHex(clientKey), pointer }))
    const signer = BunkerSigner.fromBunker(clientKey, pointer, { onauth })
    live = signer

    phase.value = 'connecting'
    await guarded(signer.connect(), CONNECT_TIMEOUT_MS, 'The signer did not answer. Check the relay in the connection string, and that the signer is online.')

    return wrap(signer)
  }

  /**
   * This tab's signer from the stored connection, for signing after sign-in (#30), or undefined
   * when there is none (another tab, or signed in with NIP-07). The signer already approved this
   * client key at sign-in, so there is no new `connect`.
   */
  function resume(): RemoteSigner | undefined {
    if (!import.meta.client) return undefined
    if (!live) {
      const stored = sessionStorage.getItem(STORAGE_KEY)
      if (!stored) return undefined
      const { clientKey, pointer } = JSON.parse(stored) as { clientKey: string, pointer: BunkerPointer }
      live = BunkerSigner.fromBunker(hexToBytes(clientKey), pointer, { onauth })
    }
    return wrap(live)
  }

  function onauth(url: string) {
    approvalUrl.value = url
    phase.value = 'awaiting-approval'
  }

  function wrap(signer: BunkerSigner): RemoteSigner {
    return {
      signEvent: async (template) => {
        phase.value = 'signing'
        approvalUrl.value = undefined
        try {
          return await guarded(signer.signEvent(template), SIGN_TIMEOUT_MS, 'The signer did not sign in time. Approve the request in your signer, then try again.')
        } finally {
          // Back to idle once answered, so a later wait (a save, an upload) starts clean.
          if (phase.value === 'signing' || phase.value === 'awaiting-approval') phase.value = 'idle'
        }
      },
      getPublicKey: () => guarded(signer.getPublicKey(), CONNECT_TIMEOUT_MS, 'The signer did not answer.')
    }
  }

  /** Stops waiting: closes the connection. The key stays only if the user is signed in. */
  async function cancel() {
    cancelLive?.()
    await close()
    sessionStorage.removeItem(STORAGE_KEY)
  }

  /** After a sign-in attempt, successful or not. */
  function settle() {
    phase.value = 'idle'
    approvalUrl.value = undefined
  }

  /** On sign-out: tell the signer, best effort, then delete the client key. */
  async function forget() {
    const stored = import.meta.client ? sessionStorage.getItem(STORAGE_KEY) : null
    if (stored && !live) {
      const { clientKey, pointer } = JSON.parse(stored) as { clientKey: string, pointer: BunkerPointer }
      live = BunkerSigner.fromBunker(hexToBytes(clientKey), pointer)
    }
    if (live) {
      await Promise.race([live.logout().catch(() => undefined), new Promise(resolve => setTimeout(resolve, 3000))])
    }
    await close()
    if (import.meta.client) sessionStorage.removeItem(STORAGE_KEY)
    settle()
  }

  async function close() {
    const signer = live
    live = undefined
    await signer?.close().catch(() => undefined)
  }

  function guarded<T>(work: Promise<T>, ms: number, timeoutMessage: string): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Nip46Timeout(timeoutMessage)), ms)
      cancelLive = () => {
        clearTimeout(timer)
        reject(new Nip46Cancelled())
      }
      work.then(resolve, reject).finally(() => {
        clearTimeout(timer)
        cancelLive = undefined
      })
    })
  }

  return { phase, approvalUrl, connect, resume, cancel, settle, forget }
}
