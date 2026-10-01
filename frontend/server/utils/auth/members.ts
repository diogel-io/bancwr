// What sign-in asks the bunker, as the `service` identity (#25): its own pubkey, and who a key is.
import type { H3Event } from 'h3'
import { decode } from 'nostr-tools/nip19'
import type { BunkerStatus, Role, TeamMember } from '#shared/types/bunker'
import { bunkerServiceFetch } from '../bunker-service'

const ROLES: Role[] = ['administrator', 'user', 'signer']

/** The bunker's own pubkey, hex. It may never sign in (ADR rule 10). */
export async function bunkerPubkey(event: H3Event): Promise<string> {
  const status = await bunkerServiceFetch<BunkerStatus>(event, '/api/bunker/status')
  const decoded = decode(status.pubkey)
  if (decoded.type !== 'npub') throw new Error('The bunker reported a pubkey that is not an npub')
  return decoded.data
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
