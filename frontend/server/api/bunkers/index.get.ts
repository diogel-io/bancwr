// SPIKE (#151): GET /api/bunkers, the bunkers this console manages, for the signed-in member: each
// one's id and label, its health, and the member's role there (none when it does not hold them).
// A bunker that does not answer is listed as unreachable rather than failing the list.
import type { BunkerStatus, TeamMember } from '#shared/types/bunker'
import { liveSession } from '../../utils/auth/session'
import { bunkerServiceFetch } from '../../utils/bunker-service'
import { configuredBunkers } from '../../utils/bunkers'

export default defineEventHandler(async (event) => {
  const session = await liveSession(event)
  if (!session) throw createError({ statusCode: 401, statusMessage: 'Not signed in', data: { error: 'not_authenticated' } })
  return await Promise.all(configuredBunkers(useRuntimeConfig(event)).map(async (bunker) => {
    const status = await bunkerServiceFetch<BunkerStatus>(event, '/api/bunker/status', { bunker }).catch(() => undefined)
    const member = status
      ? await bunkerServiceFetch<TeamMember>(event, `/api/bunker/team/by-pubkey/${session.pubkey}`, { bunker }).catch(() => undefined)
      : undefined
    return {
      id: bunker.id,
      label: bunker.label,
      status: status?.status ?? 'unreachable',
      role: member?.role ?? null,
      signedIn: Boolean(session.bunkers?.[bunker.id])
    }
  }))
})
