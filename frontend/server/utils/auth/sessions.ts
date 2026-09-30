// The server-side list of live sessions (#11). The session cookie is sealed, but a sealed cookie is
// stateless: without this list, signing out could not stop a copy of the cookie working until it
// expired. The cookie carries only a random session id; this map says whose it is and whether it
// is still live. Held in memory, like the challenges: one frontend instance, as compose.yaml runs
// it, and a restart signs everyone out.
import { randomBytes } from 'node:crypto'
import { SESSION_MAX_AGE_SECONDS } from './constants'

interface LiveSession {
  pubkey: string
  expiresAt: number
}

export class SessionRegistry {
  private readonly sessions = new Map<string, LiveSession>()

  constructor(
    private readonly maxAgeSeconds = SESSION_MAX_AGE_SECONDS,
    private readonly now: () => number = () => Math.floor(Date.now() / 1000)
  ) {}

  /** Starts a session for this pubkey and returns its id. */
  create(pubkey: string): string {
    this.sweep()
    const id = randomBytes(32).toString('hex')
    this.sessions.set(id, { pubkey, expiresAt: this.now() + this.maxAgeSeconds })
    return id
  }

  /** The session's pubkey, if the id is live. */
  pubkey(id: string | undefined): string | undefined {
    if (!id) return undefined
    const session = this.sessions.get(id)
    if (!session) return undefined
    if (session.expiresAt < this.now()) {
      this.sessions.delete(id)
      return undefined
    }
    return session.pubkey
  }

  revoke(id: string | undefined) {
    if (id) this.sessions.delete(id)
  }

  private sweep() {
    const now = this.now()
    for (const [id, session] of this.sessions) {
      if (session.expiresAt < now) this.sessions.delete(id)
    }
  }
}

export const sessionRegistry = new SessionRegistry()
