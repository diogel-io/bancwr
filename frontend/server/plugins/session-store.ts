// Keep live sessions where NUXT_SESSION_STORE_DIR says (#100), or refuse to start if the frontend
// cannot write there. See server/utils/auth/session-store.ts.
import { sessionStorage, sessionStoreProblem } from '../utils/auth/session-store'
import { sessionRegistry } from '../utils/auth/sessions'

export default defineNitroPlugin(() => {
  const dir = useRuntimeConfig().sessionStoreDir
  if (!dir) {
    // On stderr, the one console the lint rules allow: it is worth an operator's notice.
    console.error('NUXT_SESSION_STORE_DIR is not set: sessions are kept in memory, so a restart signs everyone out.')
    return
  }
  const problem = sessionStoreProblem(dir)
  if (problem) {
    throw new Error(`Bancwr cannot start:\n- ${problem}`)
  }
  sessionRegistry.useStorage(sessionStorage(dir))
  // Sessions that ended while the frontend was down. Nothing waits on it.
  sessionRegistry.sweep().catch((error: unknown) => {
    console.error('Could not sweep expired sessions:', error)
  })
})
