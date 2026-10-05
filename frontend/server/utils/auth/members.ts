// What sign-in asks the bunker, as the `service` identity (#25): its own pubkey, and who a key is.
import type { H3Event } from 'h3'
import { decode } from 'nostr-tools/nip19'
import { ROLES } from '#shared/types/bunker'
import type { BunkerStatus, Role, TeamMember } from '#shared/types/bunker'
import { bunkerServiceFetch } from '../bunker-service'

/** The bunker's own pubkey, hex. It may never sign in (ADR rule 10). */
export async function bunkerPubkey(event: H3Event): Promise<string> {
  const status = await bunkerServiceFetch<BunkerStatus>(event, '/api/bunker/status')
  const decoded = decode(status.pubkey)
  if (decoded.type !== 'npub') throw new Error('The bunker reported a pubkey that is not an npub')
  return decoded.data
}

/**
 * Whether the bunker has no administrator (#74): its `administrator` health check warns. Then
 * nobody can register keys, and an unregistered key should be told how the first administrator is
 * set (BANCWR_ADMIN_PUBKEY), not to ask an administrator who doesn't exist.
 */
export function lacksAdministrator(status: Pick<BunkerStatus, 'checks'>): boolean {
  return status.checks.some(check => check.name === 'administrator' && check.status === 'warn')
}

/** Asks the bunker whether it has no administrator. Best effort: an unreadable status says no. */
export async function bunkerLacksAdministrator(event: H3Event): Promise<boolean> {
  try {
    return lacksAdministrator(await bunkerServiceFetch<BunkerStatus>(event, '/api/bunker/status'))
  } catch {
    return false
  }
}

/** The member's role, or undefined when the key is not in the vault or holds no valid role. */
export async function memberRole(event: H3Event, pubkey: string): Promise<Role | undefined> {
  try {
    const member = await bunkerServiceFetch<TeamMember>(event, `/api/bunker/team/by-pubkey/${pubkey}`)
    return ROLES.includes(member.role as Role) ? member.role as Role : undefined
  } catch (error) {
    if ((error as { statusCode?: number }).statusCode === 404) return undefined
    throw error
  }
}
