<script setup lang="ts">
import { useFetch, computed } from '#imports'

// GET /api/bunker/metrics — served by the backend (backend/src/server.rs) but not previously used
// by the frontend.
interface Metrics {
  http_requests: number
  nip46_connections: number
  total_signatures: number
}

const { data: metrics } = await useFetch<Metrics>('/api/bunker/metrics', { key: 'bunker-metrics' })

const cards = computed(() => [
  {
    label: 'Total Signatures',
    icon: 'i-lucide-signature',
    // The dashboard used to show logs.length here, which is capped at the 100 rows the logs
    // endpoint returns and so understated the real total once a bunker passed 100 signings.
    value: metrics.value?.total_signatures ?? 0
  },
  {
    label: 'NIP-46 Connections',
    icon: 'i-lucide-plug',
    value: metrics.value?.nip46_connections ?? 0
  },
  {
    label: 'HTTP Requests',
    icon: 'i-lucide-activity',
    value: metrics.value?.http_requests ?? 0
  }
])
</script>

<template>
  <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
    <UCard
      v-for="card in cards"
      :key="card.label"
    >
      <div class="flex items-center gap-3">
        <UIcon
          :name="card.icon"
          class="size-5 shrink-0 text-primary"
        />
        <div class="min-w-0">
          <p class="text-2xl font-bold tabular-nums">
            {{ card.value }}
          </p>
          <p class="text-sm text-muted truncate">
            {{ card.label }}
          </p>
        </div>
      </div>
    </UCard>
  </div>
</template>
