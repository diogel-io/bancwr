// SPIKE (#23). POST, not GET, so nothing prefetches or caches a challenge.
import { challengeStore } from '../../utils/auth/challenges'

export default defineEventHandler(() => {
  const { challenge, expiresAt } = challengeStore.issue()
  return { challenge, expires_at: expiresAt }
})
