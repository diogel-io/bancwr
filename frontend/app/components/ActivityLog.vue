<script setup lang="ts">
import { useFetch, computed } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import { npubEncode } from 'nostr-tools/nip19'
import type { LogEntry } from '#shared/types/bunker'

const props = withDefaults(defineProps<{
  rows?: LogEntry[]
}>(), {
  rows: () => []
})

const { data: logs } = await useFetch<LogEntry[]>('/api/bunker/logs', {
  immediate: !props.rows
})

const displayLogs = computed<LogEntry[]>(() => props.rows || logs.value || [])

// Columns follow LogEntry. There is no status column: the backend logs successful signatures only.
// Member is whom the signature was for; App is the key that asked (diogel-io/workspace#38).
const columns: TableColumn<LogEntry>[] = [
  { accessorKey: 'timestamp', header: 'Timestamp' },
  { accessorKey: 'event_kind', header: 'Event Kind' },
  { id: 'member', header: 'Member' },
  { accessorKey: 'pubkey', header: 'App' }
]

const formatDate = (date: string) => {
  return new Date(date).toLocaleString()
}

/** A hex key as its npub; anything that is not a hex key as it is. */
const npubOf = (hex: string) => {
  try {
    return npubEncode(hex)
  } catch {
    return hex
  }
}

/** Shortened as connected apps show it. */
const shortNpub = (hex: string) => {
  const npub = npubOf(hex)
  return npub === hex ? hex : `${npub.slice(0, 14)}…`
}

const member = (log: LogEntry) => {
  if (log.member_name) return log.member_name
  if (log.member_pubkey) return shortNpub(log.member_pubkey)
  return 'Unknown'
}
</script>

<template>
  <UTable
    :data="displayLogs"
    :columns="columns"
  >
    <template #timestamp-cell="{ row }">
      {{ formatDate(row.original.timestamp) }}
    </template>
    <template #member-cell="{ row }">
      <span :title="row.original.member_pubkey ? npubOf(row.original.member_pubkey) : undefined">
        {{ member(row.original) }}
      </span>
    </template>
    <template #pubkey-cell="{ row }">
      <span
        class="font-mono"
        :title="npubOf(row.original.pubkey)"
      >
        {{ shortNpub(row.original.pubkey) }}
      </span>
    </template>
  </UTable>
</template>
