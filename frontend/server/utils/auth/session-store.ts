// Where live sessions are kept (#100). NUXT_SESSION_STORE_DIR names a directory, which compose
// mounts from ./sessions; empty keeps them in memory, so a restart signs everyone out.
//
// The directory is checked when the server starts (server/plugins/session-store.ts), synchronously
// so a failure stops the start, as a misconfigured sign-in does: a store the frontend cannot write
// would otherwise fail every sign-in, or quietly fall back to signing everyone out on a restart.
import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { createStorage, type Storage } from 'unstorage'
import fsDriver from 'unstorage/drivers/fs'

/** Why the frontend cannot keep sessions in this directory, or undefined when it can. */
export function sessionStoreProblem(dir: string): string | undefined {
  const probe = join(dir, `.probe-${process.pid}`)
  try {
    mkdirSync(dir, { recursive: true })
    writeFileSync(probe, '')
    rmSync(probe)
    return undefined
  } catch (error) {
    const reason = error instanceof Error && 'code' in error ? ` (${String(error.code)})` : ''
    return `NUXT_SESSION_STORE_DIR (${dir}) must be a directory the frontend can write${reason}. ` +
      'Under compose that is ./sessions, owned by UID 65532: ' +
      '`mkdir -p sessions && podman unshare chown 65532:65532 sessions` with rootless Podman, ' +
      'or `sudo chown 65532:65532 sessions` with Docker. See "Upgrading: sessions" in the README.'
  }
}

/** The store for this directory. */
export function sessionStorage(dir: string): Storage {
  return createStorage({ driver: fsDriver({ base: dir }) })
}
