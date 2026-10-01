<script setup lang="ts">
import { useFetch, computed } from '#imports'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { BunkerStatus, LogEntry } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

// Role-aware (#26): administrators get metrics, status and recent activity; users and signers the
// bunker's health only. Their view never requests the administrator-only data, which the bunker
// would refuse anyway (#25).
const auth = useAuth()
const role = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.role : undefined)
const isAdministrator = computed(() => role.value === 'administrator')

const { data: status } = await useFetch<BunkerStatus>('/api/bunker/status')
const { data: logs, error: logsError } = await useFetch<LogEntry[]>('/api/bunker/logs', {
  immediate: isAdministrator.value
})

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
        <MetricsCards v-if="isAdministrator" />
        <p
          v-else-if="role"
          data-testid="role-summary"
          class="text-sm text-muted"
        >
          Signed in as {{ ROLE_LABELS[role] }}. You can see the bunker's health here.
        </p>

        <UCard>
          <template #header>
            <h3 class="font-bold">
              Bunker Status
            </h3>
          </template>
          <div class="flex items-center gap-2">
            <div
              :class="status?.status === 'healthy' ? 'bg-success' : 'bg-error'"
              class="w-3 h-3 rounded-full animate-pulse"
            />
            <span class="capitalize">{{ status?.status || 'Unknown' }}</span>
          </div>
          <p class="text-sm text-muted mt-2 truncate">
            {{ status?.pubkey ?? '' }}
          </p>
        </UCard>

        <UCard v-if="isAdministrator">
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
          <ForbiddenNotice v-if="isForbidden(logsError)" />
          <ActivityLog
            v-else
            :rows="recentLogs"
          />
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
