<script setup lang="ts">
// Choosing a signer: a NIP-07 extension or a NIP-46 remote signer. Used to sign in (#11), and to
// reconnect the member's own signer when the page needs a signature and has none (#30).
import { npubEncode } from 'nostr-tools/nip19'
import { Nip46Cancelled, Nip46Timeout } from '~/composables/useNip46'
import type { SignerMethod } from '~/composables/useUserSigner'
import type { NostrSigner } from '~/utils/nostr-sign-in'

const props = withDefaults(defineProps<{
  /** Called with the connected signer; returns a message to show if it went wrong. */
  use: (signer: NostrSigner, method: SignerMethod) => Promise<string | undefined>
  /** When set, a signer holding any other key is refused before it signs anything (hex). */
  expectedPubkey?: string
  extensionLabel?: string
  bunkerLabel?: string
  errorTestid?: string
}>(), {
  expectedPubkey: undefined,
  extensionLabel: 'Use extension',
  bunkerLabel: 'Connect',
  errorTestid: 'signer-error'
})

const nip07 = useNip07()
const nip46 = useNip46()

const bunkerUri = ref('')
/** The key the extension will sign with, shown before its prompt so the user can check it. */
const extensionNpub = ref<string>()
const busy = ref<SignerMethod | undefined>()
const error = ref<string>()

function wrongKey(actual: string): string {
  return `This signer holds ${npubEncode(actual)}, not the key you are signed in with (${npubEncode(props.expectedPubkey!)}). Switch to that key and try again.`
}

async function withExtension() {
  const signer = nip07.signer()
  if (!signer) return
  busy.value = 'nip07'
  error.value = undefined
  extensionNpub.value = undefined
  try {
    // NIP-07 order: which key, then the signature. Showing the key first lets the user notice the
    // wrong account before approving anything.
    const pubkey = await signer.getPublicKey()
    extensionNpub.value = npubEncode(pubkey)
    if (props.expectedPubkey && pubkey !== props.expectedPubkey) {
      error.value = wrongKey(pubkey)
      return
    }
    error.value = await props.use(signer, 'nip07')
  } catch {
    // Extensions reject when the user declines, each with its own message.
    error.value = 'The extension did not sign the request. If you declined it, try again and approve it.'
  } finally {
    busy.value = undefined
  }
}

async function withBunker() {
  busy.value = 'nip46'
  error.value = undefined
  try {
    const signer = await nip46.connect(bunkerUri.value)
    if (props.expectedPubkey) {
      const pubkey = await signer.getPublicKey()
      if (pubkey !== props.expectedPubkey) {
        error.value = wrongKey(pubkey)
        await nip46.cancel()
        return
      }
    }
    error.value = await props.use(signer, 'nip46')
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
  'awaiting-approval': 'Your signer is asking you to approve this request.'
})[nip46.phase.value])
</script>

<template>
  <div class="space-y-6">
    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      :data-testid="errorTestid"
    />

    <section class="space-y-2">
      <h2 class="text-sm font-medium">
        Browser extension (NIP-07)
      </h2>
      <UButton
        v-if="nip07.available.value"
        block
        :loading="busy === 'nip07'"
        :disabled="!!busy"
        @click="withExtension"
      >
        {{ extensionLabel }}
      </UButton>
      <p
        v-if="busy === 'nip07' && extensionNpub"
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
        v-if="busy !== 'nip46'"
        block
        color="neutral"
        variant="outline"
        :disabled="!!busy || !bunkerUri"
        @click="withBunker"
      >
        {{ bunkerLabel }}
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
