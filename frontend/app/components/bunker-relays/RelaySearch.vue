<script setup lang="ts">
// Searching known relays for the bunker's list (#78): NIP-66 reports from relay monitors, or a
// built-in list when none answers. Each shows its open time and whether it asks for auth or
// payment, as the monitor reported them: hints to choose by, never checked here.
import { filterRelays, isBunkerRelayUrl } from '~/utils/relay-discovery'
import { relayKey } from '~/utils/relay-url'

const props = defineProps<{
  /** Keys (`relayKey`) of the relays already listed. */
  listed: Set<string>
  /** No room for another relay. */
  full: boolean
}>()
const emit = defineEmits<{ add: [url: string] }>()

const discovery = useRelayDiscovery()
const { state, relays, indexers } = discovery
const text = ref('')

/** The bunker only uses wss:// (or ws:// on this machine), so other reports are left out. */
const usable = computed(() => relays.value.filter(relay => isBunkerRelayUrl(relay.url)))
const shown = computed(() => filterRelays(usable.value, text.value).slice(0, 50))
const host = (url: string) => url.replace(/^wss?:\/\//u, '')
</script>

<template>
  <div
    class="space-y-3"
    data-testid="bunker-relays-search"
  >
    <div class="flex flex-wrap items-center gap-2">
      <UButton
        color="neutral"
        variant="outline"
        icon="i-lucide-search"
        :loading="state === 'searching'"
        data-testid="bunker-relays-search-start"
        @click="discovery.search()"
      >
        {{ state === 'idle' ? 'Find known relays' : 'Search again' }}
      </UButton>
      <span class="text-xs text-muted">
        From relay monitors' NIP-66 reports, read from {{ indexers.map(host).join(', ') }}.
      </span>
    </div>

    <template v-if="state === 'found' || state === 'fallback'">
      <UAlert
        v-if="state === 'fallback'"
        color="neutral"
        variant="subtle"
        icon="i-lucide-info"
        title="No relay monitor answered"
        description="These are a few well-known public relays instead. You can also add any relay by its address."
        data-testid="bunker-relays-search-fallback"
      />
      <UInput
        v-model="text"
        class="w-full"
        icon="i-lucide-filter"
        placeholder="Filter by address"
        aria-label="Filter known relays by address"
        data-testid="bunker-relays-search-filter"
      />
      <ul
        v-if="shown.length > 0"
        class="max-h-80 divide-y divide-default overflow-y-auto rounded-md border border-default"
        data-testid="bunker-relays-search-results"
      >
        <li
          v-for="relay in shown"
          :key="relay.url"
          class="flex flex-wrap items-center gap-2 px-3 py-2"
          :data-relay="relay.url"
        >
          <span class="min-w-0 flex-1 truncate font-mono text-sm">{{ relay.url }}</span>
          <UBadge
            v-if="relay.rttOpen !== undefined"
            size="sm"
            color="neutral"
            variant="subtle"
            title="Time the monitor took to open a connection"
          >
            {{ relay.rttOpen }} ms
          </UBadge>
          <UBadge
            v-if="relay.auth"
            size="sm"
            color="warning"
            variant="subtle"
          >
            Requires auth
          </UBadge>
          <UBadge
            v-if="relay.payment"
            size="sm"
            color="warning"
            variant="subtle"
          >
            Paid
          </UBadge>
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            icon="i-lucide-plus"
            :disabled="props.full || props.listed.has(relayKey(relay.url))"
            :aria-label="`Add ${relay.url}`"
            @click="emit('add', relay.url)"
          >
            {{ props.listed.has(relayKey(relay.url)) ? 'Listed' : 'Add' }}
          </UButton>
        </li>
      </ul>
      <p
        v-else
        class="text-sm text-muted"
        data-testid="bunker-relays-search-none"
      >
        No known relay matches “{{ text }}”.
      </p>
    </template>
  </div>
</template>
