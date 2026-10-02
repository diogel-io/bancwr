// Connected apps (#31) over #53's API: the connections (scoped to the caller by the bunker), and
// for administrators the tokens and the vault's members, to name whom each connection is for.
import type { IssueTokenRequest, IssueTokenResponse, Nip46Connection, Nip46Token, TeamMember } from '#shared/types/bunker'

export function useConnections() {
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')
  const isAdministrator = computed(() => auth.state.value.status === 'signed-in' && auth.state.value.role === 'administrator')

  const connections = useFetch<Nip46Connection[]>('/api/bunker/connections', { key: 'nip46-connections', default: () => [] })
  const tokens = useFetch<Nip46Token[]>('/api/bunker/connections/tokens', { key: 'nip46-tokens', default: () => [], immediate: isAdministrator.value })
  const team = useFetch<TeamMember[]>('/api/bunker/team', { key: 'nip46-team', default: () => [], immediate: isAdministrator.value })

  /** A member's name by key: "you", the vault name, or a short npub. */
  function memberName(key: string): string {
    if (key === pubkey.value) return 'you'
    const member = team.data.value?.find(m => m.pubkey === key)
    return member?.name ?? `${key.slice(0, 8)}…`
  }

  async function refresh() {
    await Promise.all([connections.refresh(), isAdministrator.value ? tokens.refresh() : undefined])
  }

  async function revoke(id: string) {
    await $fetch(`/api/bunker/connections/${encodeURIComponent(id)}`, { method: 'DELETE' })
    await refresh()
  }

  async function revokeToken(id: string) {
    await $fetch(`/api/bunker/connections/tokens/${encodeURIComponent(id)}`, { method: 'DELETE' })
    await refresh()
  }

  /** Issues a token; the response's `uri` is the only time its secret is seen. */
  async function issue(request: IssueTokenRequest): Promise<IssueTokenResponse> {
    const issued = await $fetch<IssueTokenResponse>('/api/bunker/connections/tokens', { method: 'POST', body: request })
    await refresh()
    return issued
  }

  return { pubkey, isAdministrator, connections, tokens, team, memberName, refresh, revoke, revokeToken, issue }
}
