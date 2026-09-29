// @vitest-environment node
// SPIKE (#23): the browser helper and the server verifier agree, end to end, with no HTTP server.
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
})
