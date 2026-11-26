// The identities this bunker signs as (#103). With only the primary, as on every single-key
// bunker, nothing about identities is shown anywhere: `several` is false.
import type { BunkerIdentity } from '#shared/types/bunker'

//
// With `retired`, retired identities (#146) are listed too, for an administrator, so the log can
// still name an identity that signed before it was retired. Signers get the same list either way.
export function useIdentities(options: { retired?: boolean } = {}) {
  const identities = options.retired
    ? useFetch<BunkerIdentity[]>('/api/bunker/identities', { key: 'bunker-identities-retired', query: { retired: 'true' }, default: () => [] })
    : useFetch<BunkerIdentity[]>('/api/bunker/identities', { key: 'bunker-identities', default: () => [] })
  const list = computed(() => identities.data.value ?? [])
  const several = computed(() => list.value.length > 1)

  /** An identity's label by key, or a short npub when it is not listed. */
  function label(pubkey: string | null | undefined): string {
    if (!pubkey) return list.value.find(i => i.is_primary)?.label ?? 'Primary'
    const identity = list.value.find(i => i.pubkey === pubkey)
    return identity?.label ?? `${pubkey.slice(0, 8)}…`
  }

  return { identities, list, several, label }
}
