<script setup lang="ts">
import { useFetch, computed } from '#imports'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { LogEntry } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

// Role-aware (#26): administrators get metrics, status and recent activity; users and signers the
// bunker's health only. Their view never requests the administrator-only data, which the bunker
// would refuse anyway (#25).
const auth = useAuth()
const role = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.role : undefined)
const isAdministrator = computed(() => role.value === 'administrator')

// The same polled state as the navbar indicator (#28), so the two never disagree.
const { health, ensure } = useBunkerHealth()
await ensure()
const { data: logs, error: logsError } = await useFetch<LogEntry[]>('/api/bunker/logs', {
  immediate: isAdministrator.value
})

// Yellow for degraded (#27), red for unhealthy or no answer, grey until the first answer.
const DOT: Record<string, string> = { healthy: 'bg-success', degraded: 'bg-warning', unhealthy: 'bg-error' }
const dotClass = computed(() => DOT[health.value.state] ?? 'bg-neutral-400')

const recentLogs = computed(() => (Array.isArray(logs.value) ? logs.value.slice(0, 5) : []))
</script>

<template>
  <UDashboardPanel id="dashboard">
    <template #header>
      <AppNavbar title="Dashboard" />
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
              data-testid="status-dot"
              :class="dotClass"
              class="w-3 h-3 rounded-full animate-pulse"
            />
            <span class="capitalize">{{ health.state }}</span>
          </div>
          <p class="text-sm text-muted mt-2 truncate">
            {{ health.pubkey ?? '' }}
          </p>
          <HealthChecks
            v-if="health.checks.length"
            :checks="health.checks"
            class="mt-4"
          />
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
