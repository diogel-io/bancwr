<script setup lang="ts">
// Sign in with a NIP-07 extension or a NIP-46 remote signer (#11). No nsec field: Bancwr exists so
// that nobody hands a raw private key to a web page. The signer controls are SignerConnect's, which
// the profile page reuses to reconnect (#30).
import { rememberSignerMethod, type SignerMethod } from '~/composables/useUserSigner'
import { signInWithNostr, type NostrSigner, type SignInResult } from '~/utils/nostr-sign-in'

definePageMeta({ layout: 'auth' })

const auth = useAuth()

/** What each refusal means to the person signing in. */
const REASONS: Record<string, string> = {
  wrong_url: `This page's address does not match the address Bancwr is configured with (NUXT_SITE_ORIGIN). Open Bancwr at its configured address.`,
  stale_event: 'The signature arrived too late. Try again.',
  expired_challenge: 'The sign-in request expired. Try again.',
  unknown_challenge: 'The sign-in request expired. Try again.',
  bad_signature: 'The signer returned an invalid signature.',
  wrong_kind: 'The signer signed a different kind of event than was asked for.',
  bunker_key: 'This is the bunker\'s own key, which can never sign in: the bunker signs for others, so anyone it signs for could sign in as it. Sign in with your own key; the first administrator is set with BANCWR_ADMIN_PUBKEY.'
}

async function finish(result: SignInResult, method: SignerMethod): Promise<string | undefined> {
  if (result.status === 200) {
    // So later signing (#30) knows which signer holds this key.
    rememberSignerMethod(method)
    await auth.refresh()
    await navigateTo('/')
    return undefined
  }
  if (result.status === 403 && result.error === 'not_registered') {
    await auth.refresh()
    await navigateTo('/no-access')
    return undefined
  }
  return (result.reason && REASONS[result.reason]) ?? 'Sign-in was refused. Try again.'
}

async function signIn(signer: NostrSigner, method: SignerMethod) {
  return finish(await signInWithNostr(signer, window.location.origin), method)
}
</script>

<template>
  <div class="space-y-6">
    <div>
      <h1 class="text-lg font-semibold">
        Sign in
      </h1>
      <p class="text-sm text-muted">
        Sign in with a Nostr key registered with this bunker.
      </p>
    </div>

    <SignerConnect
      :use="signIn"
      confirm-key
      extension-label="Sign in with extension"
      bunker-label="Connect and sign in"
      error-testid="sign-in-error"
    />
  </div>
</template>
