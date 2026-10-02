<script setup lang="ts">
// One connected app (#31): what it says it is (unverified), what it may sign, when it connected
// and last signed, and Revoke, confirmed in place.
import { npubEncode } from 'nostr-tools/nip19'
import type { Nip46Connection } from '#shared/types/bunker'
import { kindLabel, relativeTime, revokedReason } from '~/utils/connections'

const props = defineProps<{
  connection: Nip46Connection
  /** Whom it is for, shown to administrators. */
  forName?: string
  /** Who revoked it, as a name. */
  revokedByName?: string
}>()
const emit = defineEmits<{ revoke: [] }>()

const confirming = ref(false)
const npub = computed(() => npubEncode(props.connection.client_pubkey))
const name = computed(() => props.connection.client_name ?? `${npub.value.slice(0, 14)}…`)
const image = computed(() => props.connection.client_image ?? undefined)
const exact = (iso: string) => new Date(iso).toLocaleString()

function revoke() {
  emit('revoke')
  confirming.value = false
}
</script>

<template>
  <li
    class="flex flex-col gap-3 py-4 sm:flex-row sm:items-start"
    :data-connection="connection.id"
    :data-revoked="connection.revoked_at ? 'true' : undefined"
  >
    <UAvatar
      :src="image"
      :alt="name"
      icon="i-lucide-app-window"
      size="md"
    />
    <div class="min-w-0 flex-1 space-y-1">
      <div class="flex flex-wrap items-center gap-2">
        <p class="text-sm font-medium break-all">
          {{ name }}
        </p>
        <UTooltip
          v-if="connection.client_name || connection.client_url || connection.client_image"
          text="The app names itself; this is not checked."
        >
          <UBadge
            size="sm"
            color="warning"
            variant="subtle"
            data-testid="unverified"
          >
            Unverified
          </UBadge>
        </UTooltip>
        <UBadge
          v-if="connection.revoked_at"
          size="sm"
          color="neutral"
          variant="subtle"
        >
          Ended
        </UBadge>
      </div>
      <p
        v-if="connection.client_url"
        class="text-xs text-muted break-all"
      >
        Says it is {{ connection.client_url }}
      </p>
      <p
        class="text-xs text-muted font-mono break-all"
        :title="npub"
      >
        App key {{ npub }}
      </p>
      <p class="text-xs">
        May sign:
        <span
          v-for="kind in connection.kinds"
          :key="kind"
          class="mr-1 inline-block rounded bg-elevated px-1.5 py-0.5"
        >{{ kindLabel(kind) }}</span>
      </p>
      <p class="text-xs text-muted">
        <span :title="exact(connection.connected_at)">Connected {{ relativeTime(connection.connected_at) }}</span>
        ·
        <span
          v-if="connection.last_used_at"
          :title="exact(connection.last_used_at)"
        >last signed {{ relativeTime(connection.last_used_at) }}</span>
        <span v-else>has not signed yet</span>
        <template v-if="forName">
          · for {{ forName }}
        </template>
      </p>
      <p
        v-if="connection.revoked_at"
        class="text-xs text-muted"
        :title="exact(connection.revoked_at)"
      >
        {{ revokedReason(connection.revoked_reason) }}{{ revokedByName ? ` by ${revokedByName}` : '' }} {{ relativeTime(connection.revoked_at) }}.
      </p>
    </div>
    <div
      v-if="!connection.revoked_at"
      class="flex shrink-0 items-center gap-2"
    >
      <template v-if="confirming">
        <span class="text-xs">Revoke this app?</span>
        <UButton
          size="sm"
          color="error"
          @click="revoke"
        >
          Revoke
        </UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="ghost"
          @click="confirming = false"
        >
          Cancel
        </UButton>
      </template>
      <UButton
        v-else
        size="sm"
        color="neutral"
        variant="outline"
        icon="i-lucide-unplug"
        :aria-label="`Revoke ${name}`"
        @click="confirming = true"
      >
        Revoke
      </UButton>
    </div>
  </li>
</template>
