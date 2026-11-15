// The server-side list of live sessions (#11). The session cookie is sealed, but a sealed cookie is
// stateless: without this list, signing out could not stop a copy of the cookie working until it
// expired. The cookie carries only a random session id; this list says whose it is and whether it
// is still live.
//
// The list is kept in a store (#100): files in NUXT_SESSION_STORE_DIR, which compose puts at
// ./sessions on the host, so a restart or upgrade of the frontend signs nobody out. Each entry is
// filed under the SHA-256 of its id, never the id itself, so whoever can read the directory still
// cannot rebuild a cookie (which is sealed with NUXT_SESSION_PASSWORD besides). Without the setting
// (pnpm dev, the unit tests) the store is in memory, as it was before. One frontend instance only,
// as compose.yaml runs it.
import { createHash, randomBytes } from 'node:crypto'
import { createStorage, type Storage } from 'unstorage'
import memoryDriver from 'unstorage/drivers/memory'
import { SESSION_MAX_AGE_SECONDS } from './constants'

interface LiveSession {
  pubkey: string
  expiresAt: number
}

const STORE_KEY = /^[0-9a-f]{64}$/

/** The store key for a session id: its SHA-256, so the store never holds a usable id. */
export function sessionStoreKey(id: string): string {
  return createHash('sha256').update(id).digest('hex')
}

function liveSession(value: unknown): LiveSession | undefined {
  if (typeof value !== 'object' || value === null) return undefined
  const { pubkey, expiresAt } = value as Partial<LiveSession>
  if (typeof pubkey !== 'string' || typeof expiresAt !== 'number') return undefined
  return { pubkey, expiresAt }
}

export class SessionRegistry {
  constructor(
    private storage: Storage = createStorage({ driver: memoryDriver() }),
    private readonly maxAgeSeconds = SESSION_MAX_AGE_SECONDS,
    private readonly now: () => number = () => Math.floor(Date.now() / 1000)
  ) {}

  /** Keeps the sessions in this store from now on (the server plugin, at startup). */
  useStorage(storage: Storage) {
    this.storage = storage
  }

  /** Starts a session for this pubkey and returns its id. */
  async create(pubkey: string): Promise<string> {
    await this.sweep()
    const id = randomBytes(32).toString('hex')
    const session: LiveSession = { pubkey, expiresAt: this.now() + this.maxAgeSeconds }
    await this.storage.setItem(sessionStoreKey(id), session)
    return id
  }

  /** The session's pubkey, if the id is live. An expired or unreadable entry is removed. */
  async pubkey(id: string | undefined): Promise<string | undefined> {
    if (!id) return undefined
    const key = sessionStoreKey(id)
    const stored = await this.storage.getItem(key)
    if (stored === null) return undefined
    const session = liveSession(stored)
    if (!session || session.expiresAt < this.now()) {
      await this.storage.removeItem(key)
      return undefined
    }
    return session.pubkey
  }

  /** Ends the session. The entry is gone once this resolves, so a copy of its cookie fails. */
  async revoke(id: string | undefined) {
    if (id) await this.storage.removeItem(sessionStoreKey(id))
  }

  /** Removes expired and unreadable entries. Anything in the store that is not an entry is left. */
  async sweep() {
    const now = this.now()
    for (const key of await this.storage.getKeys()) {
      if (!STORE_KEY.test(key)) continue
      const session = liveSession(await this.storage.getItem(key))
      if (!session || session.expiresAt < now) await this.storage.removeItem(key)
    }
  }
}

export const sessionRegistry = new SessionRegistry()
