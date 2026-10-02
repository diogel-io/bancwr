<script setup lang="ts">
// When none of the member's relays held the event a page edits (#62: a profile; #32: a follow
// list): which relays were asked, a way to search one more, and why creating one here needs care.
// A replaceable event held only on relays not listed would be replaced by a new one in every app.
const props = withDefaults(defineProps<{
  /** What was not found: "profile" or "follow list". */
  noun?: string
  searched: { reached: string[], failed: string[] }
  /** Adds a relay to search; returns a message when the address is not a relay. */
  search: (url: string) => Promise<string | undefined>
}>(), { noun: 'profile' })

const relay = ref('')
const searching = ref(false)
const error = ref<string>()

async function submit() {
  searching.value = true
  error.value = await props.search(relay.value)
  searching.value = false
  if (!error.value) relay.value = ''
}

const host = (url: string) => url.replace(/^wss?:\/\//u, '').replace(/\/$/u, '')
</script>

<template>
  <div
    class="space-y-3"
    data-testid="not-found-on-relays"
  >
    <UAlert
      color="warning"
      variant="subtle"
      icon="i-lucide-search-x"
      :title="`No ${noun} found on these ${searched.reached.length + searched.failed.length} relays`"
      :description="`If you already have a ${noun}, it is on a relay not listed here. Search that relay before creating a new one: a new ${noun} replaces the old one in every app.`"
    />
    <ul
      class="flex flex-wrap gap-1.5"
      data-testid="searched-relays"
    >
      <li
        v-for="url in searched.reached"
        :key="url"
      >
        <UBadge
          color="neutral"
          variant="subtle"
          size="sm"
        >
          {{ host(url) }}
        </UBadge>
      </li>
      <li
        v-for="url in searched.failed"
        :key="url"
      >
        <UBadge
          color="error"
          variant="subtle"
          size="sm"
          :title="`${url} did not answer`"
        >
          {{ host(url) }} · no answer
        </UBadge>
      </li>
    </ul>
    <form
      class="flex flex-col gap-2 sm:flex-row sm:items-start"
      @submit.prevent="submit"
    >
      <UFormField
        label="Search another relay"
        class="flex-1"
        :error="error"
      >
        <UInput
          v-model="relay"
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
        :loading="searching"
        :disabled="!relay.trim()"
      >
        Search
      </UButton>
    </form>
  </div>
</template>
