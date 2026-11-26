// nostr-tools' own BunkerSigner.fromURI against a running bunker (#148), for the ignored test
// `nostr_tools_bunker_signer_from_uri_connects_switches_relays_and_signs`. Run from frontend/, so
// nostr-tools resolves to the frontend's copy. Prints the nostrconnect:// code on the first line,
// waits for the bunker's answer, then prints one JSON line: the signer's key, the relays it moved
// to, and whether kind 1 signed and kind 7 was refused. Node's own WebSocket is used.
import { createRequire } from 'node:module'
import { join } from 'node:path'

// Resolved from the working directory (frontend/), not from this file's.
const require = createRequire(join(process.cwd(), 'package.json'))
const { BunkerSigner, createNostrConnectURI } = require('nostr-tools/nip46')
const { generateSecretKey, getPublicKey } = require('nostr-tools/pure')

const relay = process.argv[2]
let step = 'start'
setTimeout(() => {
  console.log(JSON.stringify({ stuck: step }))
  process.exit(1)
}, 30000).unref()
const secretKey = generateSecretKey()
const secret = Math.random().toString(36).slice(2, 14)
const uri = createNostrConnectURI({ clientPubkey: getPublicKey(secretKey), relays: [relay], secret, perms: ['sign_event:1', 'sign_event:7'], name: 'nostr-tools' })
console.log(uri)

step = 'connect'
const signer = await BunkerSigner.fromURI(secretKey, uri, {}, 20000)
// fromURI gives switch_relays a second; let a slow answer land.
await new Promise(resolve => setTimeout(resolve, 1500))
step = 'get_public_key'
const pubkey = await signer.getPublicKey()
step = 'sign kind 1'
const note = await signer.signEvent({ kind: 1, content: 'from nostr-tools', tags: [], created_at: Math.floor(Date.now() / 1000) })
step = 'sign kind 7'
let kind7
try {
  await signer.signEvent({ kind: 7, content: '+', tags: [], created_at: Math.floor(Date.now() / 1000) })
  kind7 = 'signed'
} catch {
  kind7 = 'refused'
}
console.log(JSON.stringify({ pubkey, relays: signer.bp.relays, kind1: note.pubkey === pubkey, kind7 }))
await signer.close()
process.exit(0)
