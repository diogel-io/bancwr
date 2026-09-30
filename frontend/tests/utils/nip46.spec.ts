import { describe, it, expect } from 'vitest'
import { parseBunkerUri } from '~/utils/nip46'

const pubkey = 'ab'.repeat(32)

describe('parseBunkerUri', () => {
  it('reads the signer key, relays and secret', () => {
    expect(parseBunkerUri(` bunker://${pubkey.toUpperCase()}?relay=wss://a.example&relay=wss://b.example&secret=s3 `)).toEqual({
      pubkey,
      relays: ['wss://a.example', 'wss://b.example'],
      secret: 's3'
    })
  })

  it('accepts only bunker:// strings, never a NIP-05 name to look up', () => {
    for (const input of ['alice@example.com', `nostrconnect://${pubkey}?relay=wss://a.example`, pubkey, '']) {
      expect(parseBunkerUri(input), input).toHaveProperty('error')
    }
  })

  it('needs a 64-character hex key and at least one ws(s) relay', () => {
    expect(parseBunkerUri('bunker://abc?relay=wss://a.example')).toHaveProperty('error')
    expect(parseBunkerUri(`bunker://${pubkey}`)).toHaveProperty('error')
    expect(parseBunkerUri(`bunker://${pubkey}?relay=https://a.example`)).toHaveProperty('error')
  })
})
