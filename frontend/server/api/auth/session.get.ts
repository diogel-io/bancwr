// GET /api/auth/session: who is signed in, with the role read from the vault now, so a removed
// member or changed role takes effect at once. 401 not signed in; 403 not_registered with the npub
// for a signed-in key that is no longer (or never was) in the vault.
import { npubEncode } from 'nostr-tools/nip19'
import { bunkerLacksAdministrator, memberRole } from '../../utils/auth/members'
import { sessionPubkey } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  const pubkey = await sessionPubkey(event)
  if (!pubkey) {
    setResponseStatus(event, 401)
    return { error: 'not_authenticated' }
  }
  const npub = npubEncode(pubkey)
  const role = await memberRole(event, pubkey)
  if (!role) {
    setResponseStatus(event, 403)
    // With no administrator, the no-access page explains how the first one is set (#74).
    return (await bunkerLacksAdministrator(event))
      ? { error: 'not_registered', npub, noAdministrator: true }
      : { error: 'not_registered', npub }
  }
  return { pubkey, npub, role }
})
