<script setup lang="ts">
// Sign in with a NIP-07 extension or a NIP-46 remote signer (#11). No nsec field: Bancwr exists so
// that nobody hands a raw private key to a web page.
import { npubEncode } from 'nostr-tools/nip19'
import { Nip46Cancelled, Nip46Timeout } from '~/composables/useNip46'
import { signInWithNostr, type NostrSigner, type SignInResult } from '~/utils/nostr-sign-in'

definePageMeta({ layout: 'auth' })

const auth = useAuth()
const nip07 = useNip07()
const nip46 = useNip46()

const bunkerUri = ref('')
/** The key the extension will sign with, shown before its prompt so the user can check it. */
const extensionNpub = ref<string>()
const busy = ref<'extension' | 'bunker' | undefined>()
const error = ref<string>()

/** What each refusal means to the person signing in. */
const REASONS: Record<string, string> = {
  wrong_url: `This page's address does not match the address Bancwr is configured with (NUXT_SITE_ORIGIN). Open Bancwr at its configured address.`,
  stale_event: 'The signature arrived too late. Try again.',
  expired_challenge: 'The sign-in request expired. Try again.',
  unknown_challenge: 'The sign-in request expired. Try again.',
  bad_signature: 'The signer returned an invalid signature.',
  wrong_kind: 'The signer signed a different kind of event than was asked for.',
  bunker_key: 'This is the bunker\'s own key, which can never be used to sign in.'
}

async function finish(result: SignInResult) {
  if (result.status === 200) {
    await auth.refresh()
    return navigateTo('/')
  }
  if (result.status === 403 && result.error === 'not_registered') {
    await auth.refresh()
    return navigateTo('/no-access')
  }
  error.value = (result.reason && REASONS[result.reason]) ?? 'Sign-in was refused. Try again.'
}

async function withExtension() {
  const signer = nip07.signer()
  if (!signer) return
  busy.value = 'extension'
  error.value = undefined
  extensionNpub.value = undefined
  try {
    // NIP-07 order: which key, then the signature. Showing the key first lets the user notice the
    // wrong account before approving anything. The server still takes the key from the signature.
    extensionNpub.value = npubEncode(await signer.getPublicKey())
    await finish(await signInWithNostr(signer, window.location.origin))
  } catch {
    // Extensions reject when the user declines, each with its own message.
    error.value = 'The extension did not sign the request. If you declined it, try again and approve it.'
  } finally {
    busy.value = undefined
  }
}

async function withBunker() {
  busy.value = 'bunker'
  error.value = undefined
  try {
    const signer: NostrSigner = await nip46.connect(bunkerUri.value)
    await finish(await signInWithNostr(signer, window.location.origin))
  } catch (failure) {
    if (!(failure instanceof Nip46Cancelled)) {
      error.value = failure instanceof Nip46Timeout || failure instanceof Error
        ? failure.message
        : 'The remote signer could not be reached.'
    }
    await nip46.cancel()
  } finally {
    nip46.settle()
    busy.value = undefined
  }
}

async function cancel() {
  await nip46.cancel()
}

const phaseText = computed(() => ({
  'idle': '',
  'connecting': 'Connecting to your signer…',
  'signing': 'Waiting for your signer to sign…',
  'awaiting-approval': 'Your signer is asking you to approve this sign-in.'
})[nip46.phase.value])
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

    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      data-testid="sign-in-error"
    />

    <section class="space-y-2">
      <h2 class="text-sm font-medium">
        Browser extension (NIP-07)
      </h2>
      <UButton
        v-if="nip07.available.value"
        block
        :loading="busy === 'extension'"
        :disabled="!!busy"
        @click="withExtension"
      >
        Sign in with extension
      </UButton>
      <p
        v-if="busy === 'extension' && extensionNpub"
        class="text-xs text-muted break-all"
        data-testid="extension-npub"
      >
        Signing in as {{ extensionNpub }}
      </p>
      <p
        v-else-if="nip07.available.value === false"
        class="text-sm text-muted"
      >
        No Nostr signing extension was found in this browser. Install one, or use a remote signer
        below.
      </p>
    </section>

    <USeparator label="or" />

    <section class="space-y-2">
      <h2 class="text-sm font-medium">
        Remote signer (NIP-46)
      </h2>
      <UFormField
        label="Connection string"
        help="The bunker:// string your remote signer gives you."
      >
        <UInput
          v-model="bunkerUri"
          class="w-full"
          placeholder="bunker://…"
          :disabled="!!busy"
        />
      </UFormField>
      <UButton
        v-if="busy !== 'bunker'"
        block
        color="neutral"
        variant="outline"
        :disabled="!!busy || !bunkerUri"
        @click="withBunker"
      >
        Connect and sign in
      </UButton>
      <div
        v-else
        class="space-y-2"
        role="status"
      >
        <p class="text-sm">
          {{ phaseText }}
        </p>
        <UButton
          v-if="nip46.approvalUrl.value"
          :to="nip46.approvalUrl.value"
          target="_blank"
          rel="noopener noreferrer"
          variant="link"
        >
          Approve in your signer
        </UButton>
        <UButton
          block
          color="neutral"
          variant="ghost"
          @click="cancel"
        >
          Cancel
        </UButton>
      </div>
    </section>
  </div>
</template>
