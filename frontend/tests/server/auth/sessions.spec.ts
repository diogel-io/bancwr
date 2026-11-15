// @vitest-environment node
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync, chmodSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, it, expect } from 'vitest'
import { createStorage } from 'unstorage'
import memoryDriver from 'unstorage/drivers/memory'
import { SessionRegistry, sessionStoreKey } from '../../../server/utils/auth/sessions'
import { sessionStorage, sessionStoreProblem } from '../../../server/utils/auth/session-store'

const ALICE = 'a'.repeat(64)

describe('SessionRegistry', () => {
  it('resolves a live session to its pubkey', async () => {
    const registry = new SessionRegistry()
    const id = await registry.create(ALICE)
    expect(id).toMatch(/^[0-9a-f]{64}$/)
    expect(await registry.pubkey(id)).toBe(ALICE)
  })

  it('forgets a revoked session, so a copy of its cookie stops working', async () => {
    const registry = new SessionRegistry()
    const id = await registry.create(ALICE)
    await registry.revoke(id)
    expect(await registry.pubkey(id)).toBeUndefined()
  })

  it('expires a session after its maximum age', async () => {
    let now = 1_000
    const registry = new SessionRegistry(undefined, 60, () => now)
    const id = await registry.create(ALICE)
    now += 60
    expect(await registry.pubkey(id)).toBe(ALICE)
    now += 1
    expect(await registry.pubkey(id)).toBeUndefined()
  })

  it('knows nothing of an id it did not issue, or of none', async () => {
    const registry = new SessionRegistry()
    expect(await registry.pubkey('f'.repeat(64))).toBeUndefined()
    expect(await registry.pubkey(undefined)).toBeUndefined()
  })

  it('removes an entry it cannot read, and treats it as no session', async () => {
    const storage = createStorage({ driver: memoryDriver() })
    const registry = new SessionRegistry(storage)
    const id = await registry.create(ALICE)
    await storage.setItem(sessionStoreKey(id), 'not a session')
    expect(await registry.pubkey(id)).toBeUndefined()
    expect(await storage.hasItem(sessionStoreKey(id))).toBe(false)
  })

  it('sweeps expired and unreadable entries only, leaving anything else in the store', async () => {
    let now = 1_000
    const storage = createStorage({ driver: memoryDriver() })
    const registry = new SessionRegistry(storage, 60, () => now)
    const old = await registry.create(ALICE)
    now += 30
    const fresh = await registry.create(ALICE)
    await storage.setItem('b'.repeat(64), { pubkey: ALICE })
    await storage.setItem('not-an-entry', 'kept')
    now += 31
    await registry.sweep()
    expect((await storage.getKeys()).sort()).toEqual([sessionStoreKey(fresh), 'not-an-entry'].sort())
    expect(await registry.pubkey(old)).toBeUndefined()
    expect(await registry.pubkey(fresh)).toBe(ALICE)
  })
})

// A restart is a new registry on the same directory (#100).
describe('sessions kept in a directory', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'bancwr-sessions-'))
  })
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  it('keeps a session across a restart', async () => {
    const id = await new SessionRegistry(sessionStorage(dir)).create(ALICE)
    expect(await new SessionRegistry(sessionStorage(dir)).pubkey(id)).toBe(ALICE)
  })

  it('keeps a revoked session revoked across a restart', async () => {
    const before = new SessionRegistry(sessionStorage(dir))
    const id = await before.create(ALICE)
    await before.revoke(id)
    expect(await new SessionRegistry(sessionStorage(dir)).pubkey(id)).toBeUndefined()
    expect(readdirSync(dir)).toEqual([])
  })

  it('files each session under a hash of its id, and never writes the id', async () => {
    const id = await new SessionRegistry(sessionStorage(dir)).create(ALICE)
    const files = readdirSync(dir)
    expect(files).toEqual([sessionStoreKey(id)])
    expect(files[0]).not.toBe(id)
    for (const file of files) {
      expect(readFileSync(join(dir, file), 'utf8')).not.toContain(id)
    }
  })

  it('treats a half-written file as no session', async () => {
    const registry = new SessionRegistry(sessionStorage(dir))
    const id = await registry.create(ALICE)
    writeFileSync(join(dir, sessionStoreKey(id)), '{"pubkey":"aaa')
    expect(await new SessionRegistry(sessionStorage(dir)).pubkey(id)).toBeUndefined()
  })
})

describe('sessionStoreProblem', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'bancwr-sessions-'))
  })
  afterEach(() => {
    chmodSync(dir, 0o700)
    rmSync(dir, { recursive: true, force: true })
  })

  it('accepts a directory the frontend can write, creating it if needed, and leaves nothing behind', () => {
    expect(sessionStoreProblem(dir)).toBeUndefined()
    expect(readdirSync(dir)).toEqual([])
    expect(sessionStoreProblem(join(dir, 'new'))).toBeUndefined()
  })

  it.skipIf(process.getuid?.() === 0)('refuses a directory it cannot write, and says how to fix it', () => {
    chmodSync(dir, 0o500)
    const problem = sessionStoreProblem(dir)
    expect(problem).toContain('NUXT_SESSION_STORE_DIR')
    expect(problem).toContain('chown 65532:65532 sessions')
  })
})
