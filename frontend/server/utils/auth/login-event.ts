// SPIKE (#23). Verifies a sign-in request: a NIP-98 (kind 27235) event in the Authorization
// header, bound to this login URL, to POST, to the exact request body, and through that body to a
// single-use server challenge. Pure apart from the challenge store, so every rule is testable.
import { createHash } from 'node:crypto'
import { verifyEvent } from 'nostr-tools/pure'
import type { ChallengeStore } from './challenges'
import {
  CREATED_AT_FUTURE_SECONDS,
  CREATED_AT_PAST_SECONDS,
  LOGIN_EVENT_KIND,
  LOGIN_PATH
} from './constants'

export type LoginFailure =
  | 'malformed_authorization'
  | 'unknown_challenge'
  | 'expired_challenge'
  | 'wrong_kind'
  | 'bad_signature'
  | 'stale_event'
  | 'wrong_url'
  | 'wrong_method'
  | 'payload_mismatch'
  | 'bunker_key'

export class LoginError extends Error {
  constructor(readonly code: LoginFailure, readonly statusCode: 401 | 403 = 401) {
    super(code)
    this.name = 'LoginError'
  }
}

export interface LoginRequest {
  /** The Authorization header as received. */
  authorization: string | undefined
  /** The HTTP method of the request. */
  method: string
  /** The raw request body, exactly as received: the `payload` tag is its SHA-256. */
  body: string
  /** The origin users reach Bancwr on, from configuration, never from the request's Host. */
  siteOrigin: string
  /** This bunker's own pubkey (hex), which may never be used to sign in. */
  bunkerPubkey: string
  challenges: ChallengeStore
  now?: number
}

const HEX64 = /^[0-9a-f]{64}$/
const HEX128 = /^[0-9a-f]{128}$/

/** Returns the authenticated pubkey (hex), or throws LoginError. Rules are numbered as in the ADR. */
export function verifyLoginRequest(request: LoginRequest): string {
  const now = request.now ?? Math.floor(Date.now() / 1000)

  // 1. The challenge is read and consumed first, so it is single-use whatever else fails.
  let challenge: unknown
  try {
    challenge = (JSON.parse(request.body) as { challenge?: unknown }).challenge
  } catch {
    throw new LoginError('malformed_authorization')
  }
  const challengeState = typeof challenge === 'string' ? request.challenges.consume(challenge) : 'unknown'

  // 2. `Authorization: Nostr <base64(event JSON)>`, parsed into a fresh object. verifyEvent caches
  //    its result on the object it is given, so it must only ever see one parsed from the wire.
  const match = request.authorization?.match(/^Nostr ([A-Za-z0-9+/=]+)$/)
  if (!match) throw new LoginError('malformed_authorization')
  let signed: Record<string, unknown>
  try {
    signed = JSON.parse(Buffer.from(match[1]!, 'base64').toString('utf8'))
  } catch {
    throw new LoginError('malformed_authorization')
  }
  if (
    typeof signed !== 'object' || signed === null
    || typeof signed.pubkey !== 'string' || !HEX64.test(signed.pubkey)
    || typeof signed.id !== 'string' || !HEX64.test(signed.id)
    || typeof signed.sig !== 'string' || !HEX128.test(signed.sig)
    || typeof signed.created_at !== 'number' || !Array.isArray(signed.tags)
    || typeof signed.content !== 'string'
  ) {
    throw new LoginError('malformed_authorization')
  }

  // 3. Kind.
  if (signed.kind !== LOGIN_EVENT_KIND) throw new LoginError('wrong_kind')

  // 4. Signature and id.
  if (!verifyEvent(signed as unknown as Parameters<typeof verifyEvent>[0])) throw new LoginError('bad_signature')

  // 5. Freshness.
  const createdAt = signed.created_at as number
  if (createdAt < now - CREATED_AT_PAST_SECONDS || createdAt > now + CREATED_AT_FUTURE_SECONDS) {
    throw new LoginError('stale_event')
  }

  // 6–8. Bound to this URL, this method and this exact body. Exactly one of each tag.
  const tags = signed.tags as unknown[]
  const only = (name: string) => {
    const found = tags.filter((t): t is string[] => Array.isArray(t) && t[0] === name)
    return found.length === 1 ? found[0]![1] : undefined
  }
  if (only('u') !== `${request.siteOrigin}${LOGIN_PATH}`) throw new LoginError('wrong_url')
  if (request.method !== 'POST' || only('method') !== 'POST') throw new LoginError('wrong_method')
  if (only('payload') !== createHash('sha256').update(request.body, 'utf8').digest('hex')) {
    throw new LoginError('payload_mismatch')
  }

  // 9. The challenge, checked now that the event is known to be genuine.
  if (challengeState === 'expired') throw new LoginError('expired_challenge')
  if (challengeState !== 'ok') throw new LoginError('unknown_challenge')

  // 10. Never the bunker's own key: anyone the bunker will sign for could otherwise sign in as it.
  if (signed.pubkey === request.bunkerPubkey) throw new LoginError('bunker_key', 403)

  return signed.pubkey as string
}
