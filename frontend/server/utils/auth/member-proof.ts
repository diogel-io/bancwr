// SPIKE (#151): the console's half of member proof. A bunker run with BANCWR_MEMBER_PROOF=required
// accepts a member only with a session it started from the member's own NIP-98 sign-in
// (backend/src/member_proof.rs). Before sign-in, the console asks each bunker for a challenge; the
// member's one signed event carries all of them; after it, the console hands the event and the
// exact body to each bunker, and keeps the session each one starts.
import type { H3Event } from 'h3'
import { bunkerServiceFetch } from '../bunker-service'
import { configuredBunkers } from '../bunkers'

/** Each answering member-proof bunker's challenge. A bunker without member proof answers 404. */
export async function bunkerChallenges(event: H3Event): Promise<string[]> {
  const challenges = await Promise.all(configuredBunkers(useRuntimeConfig(event)).map(bunker =>
    bunkerServiceFetch<{ challenge: string }>(event, '/api/bunker/auth/challenge', { bunker, method: 'POST' })
      .then(r => r.challenge)
      .catch(() => undefined)
  ))
  return challenges.filter((c): c is string => typeof c === 'string')
}

/**
 * The session each member-proof bunker starts for this sign-in, by bunker id. A bunker that does
 * not hold the member, or has no member proof, starts none; one that refuses the event as forged
 * or replayed starts none either, and the console still signs the member in to the others.
 */
export async function bunkerSessions(event: H3Event, authorization: string, body: string): Promise<Record<string, string>> {
  const sessions: Record<string, string> = {}
  await Promise.all(configuredBunkers(useRuntimeConfig(event)).map(async (bunker) => {
    try {
      const started = await bunkerServiceFetch<{ session: string }>(event, '/api/bunker/auth/proof', {
        bunker,
        method: 'POST',
        body: { authorization, body }
      })
      sessions[bunker.id] = started.session
    } catch {
      // No member proof here (404), or not this bunker's member (403), or refused (401).
    }
  }))
  return sessions
}
