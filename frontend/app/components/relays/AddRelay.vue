<script setup lang="ts">
// Adding a relay to the member's list (#33): validated with Porwr's rules before it is staged.
import { normalizeRelayUrl, relayKey } from '~/utils/relay-url'

const props = defineProps<{ listed: Set<string> }>()
const emit = defineEmits<{ add: [url: string] }>()

const input = ref('')
const error = ref<string>()
watch(input, () => (error.value = undefined))

function add() {
  const result = normalizeRelayUrl(input.value)
  if ('error' in result) {
    error.value = result.error
    return
  }
  if (props.listed.has(relayKey(result.url))) {
    error.value = 'This relay is already in your list.'
    return
  }
  emit('add', result.url)
  input.value = ''
}
</script>

<template>
  <form
    class="flex flex-col gap-2 sm:flex-row sm:items-start"
    data-testid="add-relay"
    @submit.prevent="add"
  >
    <UFormField
      label="Add a relay"
      help="New relays are used for reading and writing; change that in the list."
      class="flex-1"
      :error="error"
    >
      <UInput
        v-model="input"
        class="w-full"
        placeholder="wss://relay.example.com"
        autocomplete="off"
      />
    </UFormField>
    <UButton
      type="submit"
      class="sm:mt-6"
      color="neutral"
      variant="outline"
      icon="i-lucide-plus"
      :disabled="!input.trim()"
    >
      Add
    </UButton>
  </form>
</template>
