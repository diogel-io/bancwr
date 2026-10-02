import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { checkedSigner, forgetSignerMethod, rememberSignerMethod, SignerMismatch, signerMethod } from '~/composables/useUserSigner'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const key = generateSecretKey()
const pubkey = getPublicKey(key)
// Fresh each time: finalizeEvent marks the object it is given as verified.
const template = (): EventTemplate => ({ kind: 0, created_at: 1, tags: [], content: '{}' })
const signingWith = (secret: Uint8Array): NostrSigner => ({ signEvent: async t => finalizeEvent(t, secret) as unknown as SignedEvent })

describe('the member\'s signer', () => {
  beforeEach(() => forgetSignerMethod())
  afterEach(() => forgetSignerMethod())

  it('passes an event signed by the session\'s key', async () => {
    const event = await checkedSigner(signingWith(key), pubkey).signEvent(template())
    expect(event.pubkey).toBe(pubkey)
  })

  it('refuses an event signed by another key', async () => {
    await expect(checkedSigner(signingWith(generateSecretKey()), pubkey).signEvent(template())).rejects.toBeInstanceOf(SignerMismatch)
  })

  it('refuses an event whose signature does not hold', async () => {
    const forged: NostrSigner = { signEvent: async t => ({ ...t, id: '0'.repeat(64), pubkey, sig: '0'.repeat(128) }) }
    await expect(checkedSigner(forged, pubkey).signEvent(template())).rejects.toBeInstanceOf(SignerMismatch)
  })

  it('is not fooled by a verified flag cached on a copied object', async () => {
    // finalizeEvent flags its argument as verified; a copy with other content keeps the flag.
    const signed = finalizeEvent(template(), key)
    const tampered: NostrSigner = { signEvent: async () => ({ ...signed, content: '{"name":"not what was signed"}' }) }
    await expect(checkedSigner(tampered, pubkey).signEvent(template())).rejects.toBeInstanceOf(SignerMismatch)
  })

  it('remembers how the member signed in, for this tab', () => {
    expect(signerMethod()).toBeUndefined()
    rememberSignerMethod('nip46')
    expect(signerMethod()).toBe('nip46')
    forgetSignerMethod()
    expect(signerMethod()).toBeUndefined()
  })
})
