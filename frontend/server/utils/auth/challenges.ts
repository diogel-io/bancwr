// Single-use login challenges (ADR rule 1 and 9), held in this process's memory. That assumes one
// frontend instance, which is how Bancwr ships (compose.yaml); several instances would need a
// shared store such as Nitro storage on Redis. See the ADR.
import { randomBytes } from 'node:crypto'
import { CHALLENGE_TTL_SECONDS } from './constants'

export class ChallengeStore {
  private readonly issued = new Map<string, number>()

  constructor(
    private readonly ttlSeconds = CHALLENGE_TTL_SECONDS,
    private readonly now: () => number = () => Math.floor(Date.now() / 1000)
  ) {}

  issue(): { challenge: string, expiresAt: number } {
    this.sweep()
    const challenge = randomBytes(32).toString('hex')
    const expiresAt = this.now() + this.ttlSeconds
    this.issued.set(challenge, expiresAt)
    return { challenge, expiresAt }
  }

  /**
   * Removes the challenge and says whether it was valid. Consumed on first presentation, even if
   * the rest of the login fails, so a captured request can never be replayed.
   */
  consume(challenge: string): 'ok' | 'unknown' | 'expired' {
    const expiresAt = this.issued.get(challenge)
    if (expiresAt === undefined) return 'unknown'
    this.issued.delete(challenge)
    return expiresAt >= this.now() ? 'ok' : 'expired'
  }

  private sweep() {
    const now = this.now()
    for (const [challenge, expiresAt] of this.issued) {
      if (expiresAt < now) this.issued.delete(challenge)
    }
  }
}

export const challengeStore = new ChallengeStore()
