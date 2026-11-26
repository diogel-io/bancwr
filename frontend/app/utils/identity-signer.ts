// A signer for one of the bunker's identities (#150), for an administrator editing its profile,
// follows and relays in the console. The editors build each event as they do for the member's own
// key; this hands it to the bunker, which signs it as the identity (`POST
// /api/bunker/identities/:pubkey/sign`, through the proxy) for those kinds and Blossom upload
// authorisations only, and logs it under the administrator. The key never leaves the bunker.
//
// Only the template is sent: never a pubkey, id or signature. What comes back is checked to be
// signed by the identity and valid before anyone uses it, as the member's own signer's is.
import { checkedSigner } from '~/composables/useUserSigner'
import { bunkerRefusal } from '~/utils/bunker-error'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

/** The bunker refused or could not sign: its reason, or a general one. */
export class IdentitySignError extends Error {}

type Post = (url: string, options: { method: 'POST', body: EventTemplate }) => Promise<SignedEvent>

export function identitySigner(pubkey: string, post: Post = (url, options) => $fetch<SignedEvent>(url, options)): NostrSigner {
  const remote: NostrSigner = {
    async signEvent(template) {
      const body: EventTemplate = { kind: template.kind, created_at: template.created_at, tags: template.tags, content: template.content }
      try {
        return await post(`/api/bunker/identities/${encodeURIComponent(pubkey)}/sign`, { method: 'POST', body })
      } catch (failure) {
        throw new IdentitySignError(bunkerRefusal(failure).message ?? 'The bunker did not sign the update, so nothing was saved. Try again.')
      }
    }
  }
  return checkedSigner(remote, pubkey)
}
