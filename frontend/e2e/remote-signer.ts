// A minimal NIP-46 remote signer for the sign-in test: kind 24133 in and out, NIP-44 v2, and
// connect / get_public_key / sign_event / ping / logout. Test use only: it signs anything asked.
// Bancwr's own NIP-46 cannot stand in (#52), and would be refused as the bunker's key anyway.
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, getPublicKey } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import * as nip44 from 'nostr-tools/nip44'

export function startRemoteSigner(nsec: string, relay: string): { pubkey: string, stop: () => void } {
  const key = decode(nsec).data as Uint8Array
  const pubkey = getPublicKey(key)
  const pool = new SimplePool()

  const subscription = pool.subscribe([relay], { kinds: [24133], '#p': [pubkey] }, {
    async onevent(event) {
      const conversation = nip44.getConversationKey(key, event.pubkey)
      const request = JSON.parse(nip44.decrypt(event.content, conversation)) as { id: string, method: string, params: string[] }
      const results: Record<string, () => string> = {
        connect: () => 'ack',
        get_public_key: () => pubkey,
        ping: () => 'pong',
        logout: () => 'ack',
        sign_event: () => JSON.stringify(finalizeEvent(JSON.parse(request.params[0]!), key))
      }
      const run = results[request.method]
      const response = finalizeEvent({
        kind: 24133,
        created_at: Math.floor(Date.now() / 1000),
        tags: [['p', event.pubkey]],
        content: nip44.encrypt(JSON.stringify(run ? { id: request.id, result: run() } : { id: request.id, error: 'unsupported' }), conversation)
      }, key)
      await Promise.any(pool.publish([relay], response))
    }
  })

  return {
    pubkey,
    stop: () => {
      subscription.close()
      pool.destroy()
    }
  }
}
