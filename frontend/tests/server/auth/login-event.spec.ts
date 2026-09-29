// @vitest-environment node
// SPIKE (#23): one case per verification rule in the decision record.
import { createHash } from 'node:crypto'
import { describe, it, expect, beforeEach } from 'vitest'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { ChallengeStore } from '../../../server/utils/auth/challenges'
import { LoginError, verifyLoginRequest, type LoginRequest } from '../../../server/utils/auth/login-event'

const ORIGIN = 'https://bancwr.example'
const URL = `${ORIGIN}/api/auth/login`
const NOW = 1_800_000_000

let clock = NOW
let challenges: ChallengeStore
const userKey = generateSecretKey()
const bunkerKey = generateSecretKey()

beforeEach(() => {
  clock = NOW
  challenges = new ChallengeStore(300, () => clock)
})

/** A valid request, with hooks to change what gets signed or sent. */
function request(options: {
  key?: Uint8Array
  kind?: number
  createdAt?: number
  tags?: (body: string) => string[][]
  body?: string
  challenge?: string
  mutate?: (event: Record<string, unknown>) => Record<string, unknown>
  authorization?: string
  method?: string
} = {}): LoginRequest {
  const challenge = options.challenge ?? challenges.issue().challenge
  const body = options.body ?? JSON.stringify({ challenge })
  const payload = createHash('sha256').update(body).digest('hex')
  const tags = options.tags?.(payload) ?? [['u', URL], ['method', 'POST'], ['payload', payload]]
  let event: Record<string, unknown> = { ...finalizeEvent({
    kind: options.kind ?? 27235,
    created_at: options.createdAt ?? NOW,
    tags,
    content: ''
  }, options.key ?? userKey) }
  if (options.mutate) event = options.mutate(event)
  return {
    authorization: options.authorization ?? `Nostr ${Buffer.from(JSON.stringify(event)).toString('base64')}`,
    method: options.method ?? 'POST',
    body,
    siteOrigin: ORIGIN,
    bunkerPubkey: getPublicKey(bunkerKey),
    challenges,
    now: clock
  }
}

function failure(req: LoginRequest) {
  try {
    verifyLoginRequest(req)
  } catch (error) {
    if (error instanceof LoginError) return { code: error.code, status: error.statusCode }
    throw error
  }
  return undefined
}

describe('verifyLoginRequest', () => {
  it('accepts a valid NIP-98 sign-in and returns the pubkey', () => {
    expect(verifyLoginRequest(request())).toBe(getPublicKey(userKey))
  })

  it('rejects a replayed request: the challenge is single-use', () => {
    const first = request()
    expect(verifyLoginRequest(first)).toBe(getPublicKey(userKey))
    expect(failure(first)).toEqual({ code: 'unknown_challenge', status: 401 })
  })

  it('consumes the challenge even when the rest of the request fails', () => {
    const { challenge } = challenges.issue()
    expect(failure(request({ challenge, kind: 1 }))?.code).toBe('wrong_kind')
    expect(failure(request({ challenge }))?.code).toBe('unknown_challenge')
  })

  it('rejects a challenge the server never issued', () => {
    expect(failure(request({ challenge: 'f'.repeat(64) }))?.code).toBe('unknown_challenge')
  })

  it('rejects an expired challenge', () => {
    const { challenge } = challenges.issue()
    clock = NOW + 301
    expect(failure(request({ challenge, createdAt: clock }))?.code).toBe('expired_challenge')
  })

  it('rejects a missing or malformed Authorization header', () => {
    expect(failure(request({ authorization: '' }))?.code).toBe('malformed_authorization')
    expect(failure(request({ authorization: 'Bearer abc' }))?.code).toBe('malformed_authorization')
    expect(failure(request({ authorization: 'Nostr bm90IGpzb24=' }))?.code).toBe('malformed_authorization')
  })

  it('rejects any kind but 27235', () => {
    expect(failure(request({ kind: 22242 }))?.code).toBe('wrong_kind')
  })

  it('rejects a tampered event', () => {
    expect(failure(request({ mutate: e => ({ ...e, created_at: (e.created_at as number) + 1 }) }))?.code).toBe('bad_signature')
  })

  it('rejects a spread copy edited after signing, despite verifyEvent\'s cache', () => {
    // finalizeEvent marks the object verified; the header carries JSON, so the mark cannot travel.
    expect(failure(request({ mutate: e => ({ ...e, tags: [['u', URL], ['method', 'POST']] }) }))?.code).toBe('bad_signature')
  })

  it('rejects an event signed too long ago or in the future', () => {
    expect(failure(request({ createdAt: NOW - 121 }))?.code).toBe('stale_event')
    expect(failure(request({ createdAt: NOW + 31 }))?.code).toBe('stale_event')
    expect(verifyLoginRequest(request({ createdAt: NOW - 120 }))).toBe(getPublicKey(userKey))
  })

  it('rejects an event for another URL or origin', () => {
    const other = (u: string) => (p: string) => [['u', u], ['method', 'POST'], ['payload', p]]
    expect(failure(request({ tags: other('https://evil.example/api/auth/login') }))?.code).toBe('wrong_url')
    expect(failure(request({ tags: other(`${ORIGIN}/api/auth/login?x=1`) }))?.code).toBe('wrong_url')
    expect(failure(request({ tags: p => [['u', URL], ['u', URL], ['method', 'POST'], ['payload', p]] }))?.code).toBe('wrong_url')
  })

  it('rejects the wrong method, in the tag or the request', () => {
    expect(failure(request({ tags: p => [['u', URL], ['method', 'GET'], ['payload', p]] }))?.code).toBe('wrong_method')
    expect(failure(request({ method: 'PUT' }))?.code).toBe('wrong_method')
  })

  it('rejects a body the payload tag does not match', () => {
    expect(failure(request({ tags: () => [['u', URL], ['method', 'POST'], ['payload', '0'.repeat(64)]] }))?.code).toBe('payload_mismatch')
  })

  it('refuses the bunker\'s own key with 403', () => {
    expect(failure(request({ key: bunkerKey }))).toEqual({ code: 'bunker_key', status: 403 })
  })
})
