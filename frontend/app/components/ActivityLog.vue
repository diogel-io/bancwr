<script setup lang="ts">
import { useFetch, computed } from '#imports'
import type { TableColumn } from '@nuxt/ui'
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
const columns: TableColumn<LogEntry>[] = [
  { accessorKey: 'timestamp', header: 'Timestamp' },
  { accessorKey: 'event_kind', header: 'Event Kind' },
  { accessorKey: 'pubkey', header: 'Member' }
]

const formatDate = (date: string) => {
  return new Date(date).toLocaleString()
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
  </UTable>
</template>
