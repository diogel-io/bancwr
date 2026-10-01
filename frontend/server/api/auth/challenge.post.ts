// POST /api/auth/challenge: a single-use challenge for the next sign-in (ADR rule 1). POST, not GET,
// so nothing prefetches or caches one.
import { challengeStore } from '../../utils/auth/challenges'

export default defineEventHandler(() => {
  const { challenge, expiresAt } = challengeStore.issue()
  return { challenge, expires_at: expiresAt }
})
