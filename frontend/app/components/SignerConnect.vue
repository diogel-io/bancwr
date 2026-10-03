<script setup lang="ts">
// Choosing a signer: a NIP-07 extension or a NIP-46 remote signer. Used to sign in (#11), and to
// reconnect the member's own signer when the page needs a signature and has none (#30).
//
// Signing in confirms the key first (#70): the signer is asked which key it holds, and nothing is
// signed until the member continues as that key. An extension decides which key signs, and some
// (Porwr) keep using the key a site first connected with, whichever is selected, so without this
// a member could sign in as a key they did not mean. Reconnecting needs no confirmation: it already
// refuses any key but the signed-in one.
import { npubEncode } from 'nostr-tools/nip19'
import { Nip46Cancelled, Nip46Timeout } from '~/composables/useNip46'
import type { SignerMethod } from '~/composables/useUserSigner'
import type { FollowProfile } from '~/utils/follows'
import { lookupKeyProfile, lookupRelays, queryRelays } from '~/utils/key-profile'
import type { NostrSigner } from '~/utils/nostr-sign-in'

const props = withDefaults(defineProps<{
  /** Called with the connected signer; returns a message to show if it went wrong. */
  use: (signer: NostrSigner, method: SignerMethod) => Promise<string | undefined>
  /** When set, a signer holding any other key is refused before it signs anything (hex). */
  expectedPubkey?: string
  /** Ask the member to confirm the signer's key before anything is signed (sign-in, #70). */
  confirmKey?: boolean
  extensionLabel?: string
  bunkerLabel?: string
  errorTestid?: string
}>(), {
  expectedPubkey: undefined,
  confirmKey: false,
  extensionLabel: 'Use extension',
  bunkerLabel: 'Connect',
  errorTestid: 'signer-error'
})

const nip07 = useNip07()
const nip46 = useNip46()
const config = useRuntimeConfig().public

const bunkerUri = ref('')
/** The key the extension will sign with, shown before its prompt so the user can check it. */
const extensionNpub = ref<string>()
const busy = ref<SignerMethod | undefined>()
const error = ref<string>()

/** A signer whose key awaits the member's confirmation (#70). */
const pending = shallowRef<{ signer: NostrSigner, method: SignerMethod, pubkey: string }>()
const pendingProfile = ref<FollowProfile>()
/** Which "use another key" guidance to show, after the member declined a key. */
const anotherKey = ref<SignerMethod>()
const origin = ref('')
onMounted(() => (origin.value = window.location.host))

const pendingNpub = computed(() => pending.value ? npubEncode(pending.value.pubkey) : '')
const pendingName = computed(() => pendingProfile.value?.name ?? `${pendingNpub.value.slice(0, 12)}…${pendingNpub.value.slice(-6)}`)

function wrongKey(actual: string): string {
  return `This signer holds ${npubEncode(actual)}, not the key you are signed in with (${npubEncode(props.expectedPubkey!)}). Switch to that key and try again.`
}

/** Thrown when a signer signs with another key than the one the member confirmed. */
class KeyChanged extends Error {}

/** The signer, refusing any event signed with a key other than `pubkey`, before it is sent. */
function confirmedSigner(signer: NostrSigner, pubkey: string): NostrSigner {
  return {
    async signEvent(template) {
      const event = await signer.signEvent(template)
      if (event.pubkey !== pubkey) {
        throw new KeyChanged(`Your signer signed with ${npubEncode(event.pubkey)}, not the key you confirmed (${npubEncode(pubkey)}), so you were not signed in.`)
      }
      return event
    }
  }
}

/** Shows the key for confirmation, and looks its name up in the background (bounded, best effort). */
function askToConfirm(signer: NostrSigner, method: SignerMethod, pubkey: string) {
  pending.value = { signer, method, pubkey }
  pendingProfile.value = undefined
  anotherKey.value = undefined
  void lookupKeyProfile(pubkey, lookupRelays(config.profileRelays, config.indexerRelays), queryRelays).then((profile) => {
    if (pending.value?.pubkey === pubkey) pendingProfile.value = profile
  })
}

async function withExtension() {
  const signer = nip07.signer()
  if (!signer) return
  busy.value = 'nip07'
  error.value = undefined
  extensionNpub.value = undefined
  anotherKey.value = undefined
  try {
    // NIP-07 order: which key, then the signature. Showing the key first lets the user notice the
    // wrong account before approving anything.
    const pubkey = await signer.getPublicKey()
    extensionNpub.value = npubEncode(pubkey)
    if (props.expectedPubkey && pubkey !== props.expectedPubkey) {
      error.value = wrongKey(pubkey)
      return
    }
    if (props.confirmKey) {
      askToConfirm(signer, 'nip07', pubkey)
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
  anotherKey.value = undefined
  try {
    const signer = await nip46.connect(bunkerUri.value)
    if (props.expectedPubkey || props.confirmKey) {
      const pubkey = await signer.getPublicKey()
      if (props.expectedPubkey && pubkey !== props.expectedPubkey) {
        error.value = wrongKey(pubkey)
        await nip46.cancel()
        return
      }
      if (props.confirmKey) {
        nip46.settle()
        askToConfirm(signer, 'nip46', pubkey)
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

/** Continue as the confirmed key: only now is anything signed. */
async function continueAsKey() {
  const waiting = pending.value
  if (!waiting) return
  busy.value = waiting.method
  error.value = undefined
  try {
    error.value = await props.use(confirmedSigner(waiting.signer, waiting.pubkey), waiting.method)
  } catch (failure) {
    if (failure instanceof KeyChanged) error.value = failure.message
    else if (failure instanceof Nip46Timeout || (waiting.method === 'nip46' && failure instanceof Error)) error.value = failure.message
    else error.value = 'The signer did not sign the request. If you declined it, try again and approve it.'
    if (waiting.method === 'nip46') await nip46.cancel()
  } finally {
    pending.value = undefined
    busy.value = undefined
    nip46.settle()
  }
}

/** Not this key: nothing was signed. Explains how to sign in with another. */
async function useAnotherKey() {
  const waiting = pending.value
  pending.value = undefined
  if (waiting?.method === 'nip46') {
    await nip46.cancel()
    bunkerUri.value = ''
  }
  anotherKey.value = waiting?.method
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

    <!-- The key the signer holds, confirmed before anything is signed (#70). -->
    <section
      v-if="pending"
      class="space-y-4 rounded-md border border-default p-4"
      data-testid="confirm-key"
    >
      <h2 class="text-sm font-medium">
        Sign in as
      </h2>
      <div class="flex items-center gap-3">
        <UAvatar
          :src="pendingProfile?.picture"
          :alt="pendingName"
          size="lg"
        />
        <div class="min-w-0">
          <p
            class="font-medium break-words"
            data-testid="confirm-name"
          >
            {{ pendingName }}
          </p>
          <p
            class="text-xs text-muted font-mono break-all"
            data-testid="confirm-npub"
          >
            {{ pendingNpub }}
          </p>
          <p
            v-if="pendingProfile?.name"
            class="text-xs text-muted"
          >
            Name from this key's public profile.
          </p>
        </div>
      </div>
      <p class="text-sm text-muted">
        Your {{ pending.method === 'nip07' ? 'extension' : 'remote signer' }} chose this key. If it is not the one you meant,
        nothing has been signed yet.
      </p>
      <div class="flex flex-col gap-2 sm:flex-row">
        <UButton
          class="flex-1 justify-center"
          :loading="!!busy"
          data-testid="confirm-continue"
          @click="continueAsKey"
        >
          Continue as {{ pendingName }}
        </UButton>
        <UButton
          class="flex-1 justify-center"
          color="neutral"
          variant="outline"
          :disabled="!!busy"
          data-testid="confirm-another"
          @click="useAnotherKey"
        >
          Use another key
        </UButton>
      </div>
    </section>

    <UAlert
      v-if="anotherKey && !pending"
      color="neutral"
      variant="subtle"
      icon="i-lucide-key-round"
      title="Signing in with another key"
      data-testid="another-key-guidance"
    >
      <template #description>
        <p v-if="anotherKey === 'nip07'">
          Your extension decides which key signs in. Some, such as Porwr, keep using the key a site
          first connected with, whichever key is selected. To use another, open your extension's
          connected sites, disconnect <strong>{{ origin }}</strong>, choose the key, and sign in again.
        </p>
        <p v-else>
          Paste the connection string of the signer that holds the key you want.
        </p>
      </template>
    </UAlert>

    <template v-if="!pending">
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
    </template>
  </div>
</template>
