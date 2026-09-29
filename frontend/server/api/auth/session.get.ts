// SPIKE (#23). Who is signed in, with the role re-read now rather than stored in the cookie.
import * as nip19 from 'nostr-tools/nip19'
import { lookupMember } from '../../utils/auth/registry'
import { useBancwrSession } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  const session = await useBancwrSession(event, useRuntimeConfig(event).sessionPassword)
  const pubkey = session.data.pubkey
  if (!pubkey) {
    setResponseStatus(event, 401)
    return { error: 'not_authenticated' }
  }
  const member = lookupMember(pubkey)
  if (!member) {
    setResponseStatus(event, 403)
    return { error: 'not_registered', npub: nip19.npubEncode(pubkey) }
  }
  return { pubkey, npub: nip19.npubEncode(pubkey), role: member.role }
})
