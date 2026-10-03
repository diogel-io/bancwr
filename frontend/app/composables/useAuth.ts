// Who is signed in, for route middleware, the sidebar and the auth pages (#11).
import type { Role } from '#shared/types/bunker'

export type AuthState =
  | { status: 'signed-in', pubkey: string, npub: string, role: Role }
  /** noAdministrator: the bunker has none yet, so nobody can register this key (#74). */
  | { status: 'not-registered', npub: string, noAdministrator?: boolean }
  | { status: 'signed-out' }

export function useAuth() {
  const state = useState<AuthState>('auth', () => ({ status: 'signed-out' }))
  const requestFetch = useRequestFetch()

  /** Re-reads the session. The role comes from the vault every time, not from the cookie. */
  async function refresh(): Promise<AuthState> {
    try {
      const session = await requestFetch<{ pubkey: string, npub: string, role: Role }>('/api/auth/session')
      state.value = { status: 'signed-in', ...session }
    } catch (error) {
      const failure = error as { statusCode?: number, data?: { error?: string, npub?: string, noAdministrator?: boolean } }
      state.value = failure.statusCode === 403 && failure.data?.error === 'not_registered' && failure.data.npub
        ? { status: 'not-registered', npub: failure.data.npub, ...(failure.data.noAdministrator ? { noAdministrator: true } : {}) }
        : { status: 'signed-out' }
    }
    return state.value
  }

  async function signOut() {
    await $fetch('/api/auth/logout', { method: 'POST' }).catch(() => undefined)
    await useNip46().forget()
    forgetSignerMethod()
    state.value = { status: 'signed-out' }
    await navigateTo('/sign-in')
  }

  return { state, refresh, signOut }
}
