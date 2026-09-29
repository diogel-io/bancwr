// The checks both nostr-tools smoke specs run: nostr-tools.spec.ts in the Nuxt (client)
// environment, nostr-tools.server.spec.ts in plain Node, as SSR would load it. See the "Nostr
// (nostr-tools)" section of the frontend README and #29.
import { expect, it } from 'vitest'
import * as root from 'nostr-tools'
import { decode, npubEncode } from 'nostr-tools/nip19'
import { finalizeEvent, generateSecretKey, getPublicKey, verifyEvent } from 'nostr-tools/pure'

export function nostrToolsSmokeTests() {
  it('round-trips a pubkey through npub', () => {
    const pubkey = getPublicKey(generateSecretKey())
    const npub = npubEncode(pubkey)

    expect(npub).toMatch(/^npub1[02-9ac-hj-np-z]{58}$/)
    expect(decode(npub)).toEqual({ type: 'npub', data: pubkey })
  })

  it('rejects a malformed npub', () => {
    expect(() => decode('npub1notbech32')).toThrow()
  })

  it('builds and verifies a kind 0 profile event', () => {
    const secretKey = generateSecretKey()
    const event = finalizeEvent({
      kind: 0,
      created_at: Math.floor(Date.now() / 1000),
      tags: [],
      content: JSON.stringify({ name: 'bancwr', about: 'smoke test' })
    }, secretKey)

    expect(event.pubkey).toBe(getPublicKey(secretKey))
    expect(event.id).toMatch(/^[0-9a-f]{64}$/)
    expect(event.sig).toMatch(/^[0-9a-f]{128}$/)
    expect(verifyEvent(event)).toBe(true)

    // A signature covers the content: change it and verification must fail. The copy goes
    // through JSON, as an event from a relay or a form would; see the next test for why.
    const tampered = { ...JSON.parse(JSON.stringify(event)), content: JSON.stringify({ name: 'mallory' }) }
    expect(verifyEvent(tampered)).toBe(false)
  })

  it('caches a verification result on the event object', () => {
    // verifyEvent and finalizeEvent store `true` under a symbol on the event, and object spread
    // copies own symbols. So a spread copy of a verified event reports valid even after its
    // content changes. Pinned here so a version bump that changes this is noticed: never
    // re-verify an edited copy of a verified event. See the frontend README.
    const event = finalizeEvent({ kind: 0, created_at: 1, tags: [], content: '{}' }, generateSecretKey())
    const edited = { ...event, content: JSON.stringify({ name: 'mallory' }) }

    expect(verifyEvent(edited)).toBe(true)
  })

  it('also resolves from the package root, as a separate copy', () => {
    // The root entry is its own bundle, not a re-export of the subpaths, so importing both ways
    // ships two copies. Bancwr imports from the subpaths only; see the frontend README.
    const pubkey = getPublicKey(generateSecretKey())

    expect(root.nip19.npubEncode(pubkey)).toBe(npubEncode(pubkey))
    expect(root.nip19.npubEncode).not.toBe(npubEncode)
  })
}
