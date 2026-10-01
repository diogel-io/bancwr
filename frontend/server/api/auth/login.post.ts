// POST /api/auth/login: sign in with a NIP-98 event (ADR "Sign-in"). 200 starts a session;
// 401 not_authenticated with a reason sends the user back to sign-in; 403 forbidden bunker_key
// refuses the bunker's own key; 403 not_registered with the npub sends them to the no-access page.
//
// A key that proves itself but is not in the vault still gets a session, one that grants nothing:
// /api/auth/session answers 403 not_registered and the bunker refuses every proxied call. Without
// it, the no-access page could not survive a navigation or offer sign-out, and a key registered
// later would have to sign in again. (Amends ADR rule 11, which said no session; see #11.)
import { npubEncode } from 'nostr-tools/nip19'
import { challengeStore } from '../../utils/auth/challenges'
import { LoginError, verifyLoginRequest } from '../../utils/auth/login-event'
import { bunkerPubkey, memberRole } from '../../utils/auth/members'
import { startSession } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)

  let pubkey: string
  try {
    pubkey = verifyLoginRequest({
      authorization: getHeader(event, 'authorization'),
      method: event.method,
      body: (await readRawBody(event, 'utf8')) ?? '',
      siteOrigin: config.siteOrigin,
      bunkerPubkey: await bunkerPubkey(event),
      challenges: challengeStore
    })
  } catch (error) {
    if (error instanceof LoginError) {
      setResponseStatus(event, error.statusCode)
      return error.statusCode === 401
        ? { error: 'not_authenticated', reason: error.code }
        : { error: 'forbidden', reason: error.code }
    }
    throw error
  }

  const npub = npubEncode(pubkey)
  await startSession(event, pubkey)
  const role = await memberRole(event, pubkey)
  if (!role) {
    setResponseStatus(event, 403)
    return { error: 'not_registered', npub }
  }
  return { pubkey, npub, role }
})
