// POST /api/auth/challenge: a single-use challenge for the next sign-in (ADR rule 1). POST, not GET,
// so nothing prefetches or caches one.
//
// SPIKE (#151): also each member-proof bunker's own challenge, as `proofs`. The sign-in body
// carries them, so the member's one signature covers every bunker.
import { challengeStore } from '../../utils/auth/challenges'
import { bunkerChallenges } from '../../utils/auth/member-proof'

export default defineEventHandler(async (event) => {
  const { challenge, expiresAt } = challengeStore.issue()
  return { challenge, expires_at: expiresAt, proofs: await bunkerChallenges(event) }
})
