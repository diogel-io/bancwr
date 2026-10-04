<script setup lang="ts">
// Adding a follow (#32): an npub, hex key or NIP-05 identifier, resolved to one key and shown
// with its name and picture before it is staged.
import { npubEncode } from 'nostr-tools/nip19'
import { displayName, validateFollowInput, type FollowProfile } from '~/utils/follows'
import { parseNip05Identifier, resolveNip05, type Nip05Lookup } from '~/utils/nip05'
import { NoReferrerImg } from '~/utils/no-referrer-img'

// Third-party pictures load with no referrer, the policy set before src (#75).
const avatarAs = { img: NoReferrerImg }
const props = defineProps<{
  /** Keys already followed or staged. */
  following: Set<string>
  self: string
  profiles: Record<string, FollowProfile>
  loadProfiles: (keys: string[]) => Promise<void>
}>()
const emit = defineEmits<{ add: [pubkey: string] }>()

const input = ref('')
const finding = ref(false)
const error = ref<string>()
const candidate = ref<string>()

const NIP05_ERRORS: Record<Exclude<Nip05Lookup, { pubkey: string }>['status'], string> = {
  'malformed': 'Enter an identifier like name@example.com.',
  'not-found': 'That domain does not list this name.',
  'network-error': 'That domain could not be reached, or does not allow lookups from a browser.',
  'invalid-response': 'That domain\'s answer is not a valid NIP-05 response.'
}

watch(input, () => {
  error.value = undefined
  candidate.value = undefined
})

async function find() {
  const value = input.value.trim()
  if (!value) return
  finding.value = true
  error.value = undefined
  try {
    let key = value
    if (value.includes('@') && parseNip05Identifier(value)) {
      const lookup = await resolveNip05(value)
      if ('status' in lookup) {
        error.value = NIP05_ERRORS[lookup.status]
        return
      }
      key = lookup.pubkey
    }
    const result = validateFollowInput(key, props.following, props.self)
    if ('error' in result) {
      error.value = result.error
      return
    }
    candidate.value = result.pubkey
    await props.loadProfiles([result.pubkey])
  } finally {
    finding.value = false
  }
}

function add() {
  if (!candidate.value) return
  emit('add', candidate.value)
  input.value = ''
}

const preview = computed(() => candidate.value
  ? { name: displayName({ pubkey: candidate.value, tag: ['p', candidate.value] }, props.profiles[candidate.value]), profile: props.profiles[candidate.value], npub: npubEncode(candidate.value) }
  : undefined)
</script>

<template>
  <div
    class="space-y-3"
    data-testid="add-follow"
  >
    <form
      class="flex flex-col gap-2 sm:flex-row sm:items-start"
      @submit.prevent="find"
    >
      <UFormField
        label="Follow someone"
        help="An npub, a hex key, or a NIP-05 identifier like name@example.com."
        class="flex-1"
        :error="error"
      >
        <UInput
          v-model="input"
          class="w-full"
          placeholder="npub1… or name@example.com"
          autocomplete="off"
        />
      </UFormField>
      <UButton
        type="submit"
        class="sm:mt-6"
        color="neutral"
        variant="outline"
        :loading="finding"
        :disabled="!input.trim()"
      >
        Find
      </UButton>
    </form>

    <div
      v-if="preview"
      class="flex items-center gap-3 rounded-md border border-default p-3"
      data-testid="add-follow-preview"
    >
      <UAvatar
        :src="preview.profile?.picture"
        :alt="preview.name"
        :as="avatarAs"
        size="md"
      />
      <div class="min-w-0 flex-1">
        <p class="text-sm font-medium truncate">
          {{ preview.name }}
        </p>
        <p class="text-xs text-muted font-mono truncate">
          {{ preview.npub }}
        </p>
      </div>
      <UButton
        icon="i-lucide-user-plus"
        @click="add"
      >
        Add
      </UButton>
    </div>
  </div>
</template>
