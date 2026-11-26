// Connected apps (#31) over #53's API: the connections and tokens (scoped to the caller by the
// bunker: a signer's own, #142), and for administrators the vault's members, to name whom each
// connection is for.
import type { IssueTokenRequest, IssueTokenResponse, Nip46Connection, Nip46Token, NostrConnectPreview, NostrConnectRequest, TeamMember } from '#shared/types/bunker'

export function useConnections() {
  const auth = useAuth()
  const pubkey = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')
  const isAdministrator = computed(() => auth.state.value.status === 'signed-in' && auth.state.value.role === 'administrator')

  const connections = useFetch<Nip46Connection[]>('/api/bunker/connections', { key: 'nip46-connections', default: () => [] })
  const tokens = useFetch<Nip46Token[]>('/api/bunker/connections/tokens', { key: 'nip46-tokens', default: () => [] })
  const team = useFetch<TeamMember[]>('/api/bunker/team', { key: 'nip46-team', default: () => [], immediate: isAdministrator.value })

  /** A member's name by key: "you", the vault name, or a short npub. */
  function memberName(key: string): string {
    if (key === pubkey.value) return 'you'
    const member = team.data.value?.find(m => m.pubkey === key)
    return member?.name ?? `${key.slice(0, 8)}…`
  }

  async function refresh() {
    await Promise.all([connections.refresh(), tokens.refresh()])
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

  /** Reads an app's nostrconnect:// code (#148); changes nothing. */
  async function preview(uri: string): Promise<NostrConnectPreview> {
    return await $fetch<NostrConnectPreview>('/api/bunker/connections/nostrconnect/preview', { method: 'POST', body: { uri } })
  }

  /** Connects the app in a nostrconnect:// code (#148): the bunker answers it on its relays. */
  async function connectApp(request: NostrConnectRequest): Promise<Nip46Connection> {
    const connection = await $fetch<Nip46Connection>('/api/bunker/connections/nostrconnect', { method: 'POST', body: request })
    await refresh()
    return connection
  }

  return { pubkey, isAdministrator, connections, tokens, team, memberName, refresh, revoke, revokeToken, issue, preview, connectApp }
}
