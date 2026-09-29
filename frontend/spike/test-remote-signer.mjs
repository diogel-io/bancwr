// SPIKE (#23). A minimal NIP-46 remote signer (bunker) for the round-trip test: kind 24133 in and
// out, NIP-44 v2, connect / get_public_key / sign_event / ping. Test use only: it signs anything.
// Usage: SIGNER_NSEC=nsec1… RELAY=ws://127.0.0.1:7777 node spike/test-remote-signer.mjs
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, getPublicKey } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import * as nip44 from 'nostr-tools/nip44'

const key = decode(process.env.SIGNER_NSEC).data
const pubkey = getPublicKey(key)
const relay = process.env.RELAY ?? 'ws://127.0.0.1:7777'
const pool = new SimplePool()

pool.subscribe([relay], { kinds: [24133], '#p': [pubkey], since: Math.floor(Date.now() / 1000) - 5 }, {
  async onevent(event) {
    const conversation = nip44.getConversationKey(key, event.pubkey)
    const request = JSON.parse(nip44.decrypt(event.content, conversation))
    let result
    switch (request.method) {
      case 'connect': result = 'ack'; break
      case 'get_public_key': result = pubkey; break
      case 'ping': result = 'pong'; break
      case 'sign_event': result = JSON.stringify(finalizeEvent(JSON.parse(request.params[0]), key)); break
      default: result = undefined
    }
    const response = finalizeEvent({
      kind: 24133,
      created_at: Math.floor(Date.now() / 1000),
      tags: [['p', event.pubkey]],
      content: nip44.encrypt(JSON.stringify(result === undefined ? { id: request.id, error: 'unsupported' } : { id: request.id, result }), conversation)
    }, key)
    await Promise.any(pool.publish([relay], response))
    process.stdout.write(`answered ${request.method}\n`)
  }
})
process.stdout.write(`test remote signer ${pubkey} on ${relay}\n`)
