<script setup lang="ts">
// When no profile was found for the member's key (#62): which relays were asked, a way to search
// one more, and why creating a profile here needs care. A profile held only on relays not listed
// would be replaced by a new one in every app.
const props = defineProps<{
  searched: { reached: string[], failed: string[] }
  /** Adds a relay to search; returns a message when the address is not a relay. */
  search: (url: string) => Promise<string | undefined>
}>()

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
    data-testid="profile-not-found"
  >
    <UAlert
      color="warning"
      variant="subtle"
      icon="i-lucide-search-x"
      :title="`No profile found on these ${searched.reached.length + searched.failed.length} relays`"
      description="If you already have a profile, it is on a relay not listed here. Search that relay before creating a new one: a new profile replaces the old one in every app."
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
