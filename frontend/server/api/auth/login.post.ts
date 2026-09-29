// SPIKE (#23). Sign-in: verify the NIP-98 event, look the key up, start a session.
import * as nip19 from 'nostr-tools/nip19'
import { challengeStore } from '../../utils/auth/challenges'
import { LoginError, verifyLoginRequest } from '../../utils/auth/login-event'
import { lookupMember } from '../../utils/auth/registry'
import { useBancwrSession } from '../../utils/auth/session'
import { resolveBackendUrl } from '../../utils/backend'

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  if (!config.siteOrigin) {
    throw createError({ statusCode: 500, statusMessage: 'NUXT_SITE_ORIGIN must be set' })
  }

  // The bunker's own pubkey, which may not sign in. Its status route is open today; under #25 this
  // call carries a proxy signature as a service identity.
  const status = await $fetch<{ pubkey: string }>(`${resolveBackendUrl(config.apiBase)}/api/bunker/status`)
  const bunker = nip19.decode(status.pubkey)
  const bunkerPubkey = bunker.type === 'npub' ? bunker.data : ''

  let pubkey: string
  try {
    pubkey = verifyLoginRequest({
      authorization: getHeader(event, 'authorization'),
      method: event.method,
      body: (await readRawBody(event, 'utf8')) ?? '',
      siteOrigin: config.siteOrigin,
      bunkerPubkey,
      challenges: challengeStore
    })
  } catch (error) {
    if (error instanceof LoginError) {
      setResponseStatus(event, error.statusCode)
      return { error: error.statusCode === 401 ? 'not_authenticated' : 'forbidden', reason: error.code }
    }
    throw error
  }

  const npub = nip19.npubEncode(pubkey)
  const member = lookupMember(pubkey)
  if (!member) {
    // Authenticated but not registered: #11 routes this to the no-access page, not to sign-in.
    setResponseStatus(event, 403)
    return { error: 'not_registered', npub }
  }

  // A new session for a new sign-in: clear first, so no earlier state survives.
  const session = await useBancwrSession(event, config.sessionPassword)
  await session.clear()
  await session.update({ pubkey, authenticatedAt: Math.floor(Date.now() / 1000) })
  return { pubkey, npub, role: member.role }
})
