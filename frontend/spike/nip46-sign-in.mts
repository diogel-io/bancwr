// SPIKE (#23). Signs in over NIP-46 through a relay, measuring each leg, then uses the session.
// Usage: SIGNER_HEX=<remote signer pubkey> RELAY=ws://127.0.0.1:7777 ORIGIN=http://localhost:3300 \
//          node spike/nip46-sign-in.mts
import { BunkerSigner } from 'nostr-tools/nip46'
import { SimplePool } from 'nostr-tools/pool'
import { generateSecretKey } from 'nostr-tools/pure'
import { signInWithNostr } from '../app/utils/nostr-sign-in.ts'

const origin = process.env.ORIGIN!
const pool = new SimplePool()
// The NIP-46 client keypair: disposable, browser-side only, never part of the Bancwr session.
const signer = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: process.env.SIGNER_HEX!, relays: [process.env.RELAY!], secret: null }, { pool })

let t = performance.now()
const lap = () => { const d = performance.now() - t; t = performance.now(); return `${d.toFixed(0)} ms` }
await signer.connect(); process.stdout.write(`connect        ${lap()}\n`)
const pubkey = await signer.getPublicKey(); process.stdout.write(`get_public_key ${lap()}  (${pubkey.slice(0, 12)}…)\n`)

// Keep the session cookie between requests, as a browser would.
let cookie = ''
const cookieFetch = (async (url: string, init: RequestInit = {}) => {
  const response = await fetch(url, { ...init, headers: { ...(init.headers as Record<string, string>), ...(cookie ? { cookie } : {}) } })
  const set = response.headers.get('set-cookie')
  if (set) cookie = set.split(';')[0]!
  return response
}) as typeof fetch

const signingStart = performance.now()
const result = await signInWithNostr({ signEvent: e => signer.signEvent(e) }, origin, cookieFetch)
process.stdout.write(`sign-in total  ${(performance.now() - signingStart).toFixed(0)} ms (challenge + sign_event round trip + login)\n`)
process.stdout.write(`login          ${JSON.stringify(result)}\n`)

const whoami = await cookieFetch(`${origin}/api/bunker/whoami`)
process.stdout.write(`whoami         ${whoami.status} ${await whoami.text()}\n`)
await cookieFetch(`${origin}/api/auth/logout`, { method: 'POST' })
const after = await cookieFetch(`${origin}/api/bunker/whoami`)
process.stdout.write(`after logout   ${after.status}\n`)

await signer.close(); pool.destroy(); process.exit(0)
