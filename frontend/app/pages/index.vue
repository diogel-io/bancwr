<script setup lang="ts">
import { useFetch, computed } from '#imports'

const { data: status } = await useFetch('/api/bunker/status')
const { data: logs } = await useFetch('/api/bunker/logs')

const recentLogs = computed(() => (Array.isArray(logs.value) ? logs.value.slice(0, 5) : []))
</script>

<template>
  <UDashboardPanel id="dashboard">
    <template #header>
      <UDashboardNavbar title="Dashboard">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="space-y-6">
        <MetricsCards />

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
  </UDashboardPanel>
</template>
