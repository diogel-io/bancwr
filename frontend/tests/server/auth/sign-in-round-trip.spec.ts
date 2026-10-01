// @vitest-environment node
// The browser helper and the server verifier agree, end to end, with no HTTP server (#11).
import { describe, it, expect } from 'vitest'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { signInWithNostr, type NostrSigner } from '../../../app/utils/nostr-sign-in'
import { ChallengeStore } from '../../../server/utils/auth/challenges'
import { LoginError, verifyLoginRequest } from '../../../server/utils/auth/login-event'

const ORIGIN = 'https://bancwr.example'

function fakeServer(bunkerPubkey: string) {
  const challenges = new ChallengeStore()
  return (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input)
    if (url.endsWith('/api/auth/challenge')) return Response.json(challenges.issue())
    try {
      const pubkey = verifyLoginRequest({
        authorization: new Headers(init?.headers).get('authorization') ?? undefined,
        method: init?.method ?? 'GET',
        body: String(init?.body ?? ''),
        siteOrigin: ORIGIN,
        bunkerPubkey,
        challenges
      })
      return Response.json({ pubkey, npub: 'n/a', role: 'administrator' })
    } catch (error) {
      if (!(error instanceof LoginError)) throw error
      return Response.json({ error: 'not_authenticated', reason: error.code }, { status: error.statusCode })
    }
  }) as typeof fetch
}

const signerFor = (key: Uint8Array): NostrSigner => ({
  signEvent: async template => finalizeEvent(template, key)
})

describe('signInWithNostr against verifyLoginRequest', () => {
  it('signs in', async () => {
    const key = generateSecretKey()
    const result = await signInWithNostr(signerFor(key), ORIGIN, fakeServer('0'.repeat(64)))
    expect(result).toMatchObject({ status: 200, pubkey: getPublicKey(key) })
  })

  it('is refused when the client names another origin', async () => {
    const result = await signInWithNostr(signerFor(generateSecretKey()), 'https://evil.example', fakeServer('0'.repeat(64)))
    expect(result).toMatchObject({ status: 401, reason: 'wrong_url' })
  })

  it('retries once with a fresh challenge when the first attempt was too slow', async () => {
    const key = generateSecretKey()
    const now = Math.floor(Date.now() / 1000)
    let signings = 0
    const slowOnce: NostrSigner = {
      // The first signature is backdated past the 120 s window, as a slow approval would be.
      signEvent: async template => finalizeEvent({ ...template, created_at: signings++ === 0 ? now - 600 : now }, key)
    }
    const result = await signInWithNostr(slowOnce, ORIGIN, fakeServer('0'.repeat(64)))
    expect(result).toMatchObject({ status: 200 })
    expect(signings).toBe(2)
  })

  it('does not retry a refusal that a new challenge cannot cure', async () => {
    let signings = 0
    const signer: NostrSigner = { signEvent: async template => { signings++; return finalizeEvent(template, generateSecretKey()) } }
    await signInWithNostr(signer, 'https://evil.example', fakeServer('0'.repeat(64)))
    expect(signings).toBe(1)
  })
})
