<script setup lang="ts">
// The NIP-05 Identity Verification section (#30), after Porwr's brief
// (porwr/design/profile-nip05-identity-card-implementation-brief.md). Its invariants:
// verification runs only when asked, never on input; the status resets when the value changes;
// Verify is disabled for a malformed identifier; and a result that arrives after the value has
// changed is dropped (porwr/design/profile-nip05-stale-async-result-bug.md).
import { parseNip05Identifier, verifyNip05, type Nip05Result } from '~/utils/nip05'

const props = defineProps<{ pubkey: string }>()
const identifier = defineModel<string>({ required: true })

const verifying = ref(false)
const result = ref<Nip05Result>()
/** Bumped by every new request and every edit, so only the latest request may report. */
let request = 0

const parsed = computed(() => parseNip05Identifier(identifier.value))
const canVerify = computed(() => !!parsed.value && !verifying.value)

watch(identifier, () => {
  request++
  verifying.value = false
  result.value = undefined
})

async function verify() {
  const value = identifier.value.trim()
  if (!parseNip05Identifier(value)) return
  const mine = ++request
  verifying.value = true
  try {
    const outcome = await verifyNip05(value, props.pubkey)
    if (mine !== request || identifier.value.trim() !== value) return
    result.value = outcome
  } finally {
    if (mine === request) verifying.value = false
  }
}

const FAILURES: Record<string, string> = {
  'pubkey-mismatch': 'This identifier resolves to a different public key.',
  'not-found': 'The domain does not list this name.',
  'network-error': 'The domain could not be reached, or does not allow verification from a browser.',
  'invalid-response': 'The domain\'s answer is not a valid NIP-05 response.',
  'malformed': 'Enter an identifier like name@example.com.'
}

const tone = computed(() => {
  if (verifying.value) return 'verifying'
  if (!result.value) return 'neutral'
  return result.value.status === 'verified' ? 'verified' : 'failed'
})
</script>

<template>
  <section
    class="space-y-3 rounded-md border border-default p-4"
    aria-labelledby="identity-verification-title"
  >
    <div class="flex items-center gap-2">
      <h3
        id="identity-verification-title"
        class="text-sm font-semibold"
      >
        Identity Verification
      </h3>
      <UBadge
        size="sm"
        color="neutral"
        variant="subtle"
      >
        NIP-05
      </UBadge>
    </div>
    <p class="text-sm text-muted">
      Prove that a domain maps this identifier to your Nostr key.
    </p>

    <div class="flex flex-col gap-2 sm:flex-row sm:items-start">
      <UFormField
        label="NIP-05 identifier"
        class="flex-1"
      >
        <UInput
          v-model="identifier"
          class="w-full"
          placeholder="name@example.com"
          autocomplete="off"
        />
      </UFormField>
      <UButton
        class="sm:mt-6"
        color="neutral"
        variant="outline"
        :loading="verifying"
        :disabled="!canVerify"
        data-testid="verify-nip05"
        @click="verify"
      >
        Verify identifier
      </UButton>
    </div>

    <div
      role="status"
      data-testid="nip05-status"
      :data-tone="tone"
      class="text-sm"
    >
      <p
        v-if="tone === 'verifying'"
        class="text-muted"
      >
        Verifying…
      </p>
      <div
        v-else-if="tone === 'verified'"
        class="flex items-start gap-2 text-success"
      >
        <UIcon
          name="i-lucide-badge-check"
          class="size-4 mt-0.5 shrink-0"
          aria-hidden="true"
        />
        <p>
          <span class="font-medium">Verified</span><br>
          <span class="text-default">{{ result?.domain }} returned this account's public key.</span>
        </p>
      </div>
      <div
        v-else-if="tone === 'failed'"
        class="flex items-start gap-2 text-error"
      >
        <UIcon
          name="i-lucide-circle-alert"
          class="size-4 mt-0.5 shrink-0"
          aria-hidden="true"
        />
        <p>
          <span class="font-medium">Unable to verify this identifier.</span><br>
          <span class="text-default">{{ FAILURES[result!.status] }}</span>
        </p>
      </div>
      <p
        v-else
        class="text-muted"
      >
        {{ identifier.trim() && !parsed ? FAILURES.malformed : 'Not verified yet. Verification runs only when you ask.' }}
      </p>
    </div>
  </section>
</template>
