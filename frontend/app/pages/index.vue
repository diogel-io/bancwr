<script setup lang="ts">
import { useFetch, computed } from '#imports'

const { data: status } = await useFetch('/api/bunker/status')
const { data: logs } = await useFetch('/api/bunker/logs')

const recentLogs = computed(() => (Array.isArray(logs.value) ? logs.value.slice(0, 5) : []))
</script>

<template>
  <div class="space-y-8">
    <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
      <UCard>
        <template #header>
          <h3 class="font-bold">
            Bunker Status
          </h3>
        </template>
        <div class="flex items-center gap-2">
          <div
            :class="status && status['status'] === 'healthy' ? 'bg-success' : 'bg-error'"
            class="w-3 h-3 rounded-full animate-pulse"
          />
          <span class="capitalize">{{ status ? status['status'] || 'Unknown' : 'Unknown' }}</span>
        </div>
        <p class="text-sm text-muted mt-2 truncate">
          {{ status ? status['pubkey'] : '' }}
        </p>
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-bold">
            Quick Actions
          </h3>
        </template>
        <div class="flex flex-col gap-2">
          <UButton
            to="/config"
            variant="soft"
            block
          >
            Configure Bunker
          </UButton>
          <UButton
            to="/team"
            variant="soft"
            block
          >
            Manage Team
          </UButton>
        </div>
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-bold">
            Stats
          </h3>
        </template>
        <p class="text-3xl font-bold">
          {{ logs && Array.isArray(logs) ? logs.length : 0 }}
        </p>
        <p class="text-sm text-muted">
          Total Signing Events
        </p>
      </UCard>
    </div>

    <UCard>
      <template #header>
        <div class="flex items-center justify-between">
          <h3 class="font-bold">
            Recent Activity
          </h3>
          <UButton
            to="/logs"
            variant="link"
            color="neutral"
            size="xs"
          >
            View All
          </UButton>
        </div>
      </template>
      <ActivityLog :rows="recentLogs" />
    </UCard>
  </div>
</template>
