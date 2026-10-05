<script setup lang="ts">
import { useFetch, computed } from '#imports'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { LogEntry, Nip46Connection } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'
import { relativeTime } from '~/utils/connections'

// Role-aware (#26, #77). Every role sees the bunker's health. Administrators also get metrics and
// everyone's recent activity; signers their connected apps and their own recent signatures
// (logs/mine); viewers a way to the team. Each view requests only its own role's data, which the
// bunker would refuse to the others anyway (#25).
const auth = useAuth()
const role = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.role : undefined)
const isAdministrator = computed(() => role.value === 'administrator')
const isSigner = computed(() => role.value === 'signer')
const isViewer = computed(() => role.value === 'viewer')

// The same polled state as the navbar indicator (#28), so the two never disagree.
const { health, ensure } = useBunkerHealth()
await ensure()
const { data: logs, error: logsError } = await useFetch<LogEntry[]>('/api/bunker/logs', {
  immediate: isAdministrator.value
})
// The signer's own: the bunker scopes both to the caller.
const { data: myConnections, error: connectionsError } = await useFetch<Nip46Connection[]>('/api/bunker/connections', {
  key: 'dashboard-connections',
  immediate: isSigner.value
})
const { data: myLogs, error: myLogsError } = await useFetch<LogEntry[]>('/api/bunker/logs/mine', {
  immediate: isSigner.value
})

// Yellow for degraded (#27), red for unhealthy or no answer, grey until the first answer.
const DOT: Record<string, string> = { healthy: 'bg-success', degraded: 'bg-warning', unhealthy: 'bg-error' }
const dotClass = computed(() => DOT[health.value.state] ?? 'bg-neutral-400')

const recentLogs = computed(() => (Array.isArray(logs.value) ? logs.value.slice(0, 5) : []))
const myRecentLogs = computed(() => (Array.isArray(myLogs.value) ? myLogs.value.slice(0, 5) : []))

const activeConnections = computed(() => (Array.isArray(myConnections.value) ? myConnections.value : []).filter(c => !c.revoked_at))
/** The most recently connected of the signer's active apps. */
const latestConnection = computed(() => [...activeConnections.value]
  .sort((a, b) => new Date(b.connected_at).getTime() - new Date(a.connected_at).getTime())[0])
const appName = (connection: Nip46Connection) => connection.client_name ?? `${connection.client_pubkey.slice(0, 8)}…`
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
          Signed in as {{ ROLE_LABELS[role] }}.
          <template v-if="isSigner">
            Your connected apps and the signatures made for you are below.
          </template>
          <template v-else>
            You can see the bunker's health and the team.
          </template>
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

        <template v-if="isSigner">
          <UCard data-testid="my-connections">
            <template #header>
              <div class="flex items-center justify-between">
                <h3 class="font-bold">
                  Your Connected Apps
                </h3>
                <UButton
                  to="/connections"
                  variant="link"
                  color="neutral"
                  size="xs"
                >
                  Manage
                </UButton>
              </div>
            </template>
            <ForbiddenNotice v-if="isForbidden(connectionsError)" />
            <div
              v-else
              class="space-y-1"
            >
              <p
                class="text-2xl font-bold"
                data-testid="my-connections-count"
              >
                {{ activeConnections.length }}
              </p>
              <p
                v-if="latestConnection"
                class="text-sm text-muted"
                data-testid="my-latest-connection"
              >
                Latest: <strong class="text-default">{{ appName(latestConnection) }}</strong>,
                connected {{ relativeTime(latestConnection.connected_at) }}
              </p>
              <p
                v-else
                class="text-sm text-muted"
              >
                No apps are connected for you. Ask an administrator for a connection token.
              </p>
            </div>
          </UCard>

          <UCard data-testid="my-signatures">
            <template #header>
              <h3 class="font-bold">
                Your Recent Signatures
              </h3>
            </template>
            <ForbiddenNotice v-if="isForbidden(myLogsError)" />
            <ActivityLog
              v-else
              :rows="myRecentLogs"
            />
          </UCard>
        </template>

        <UCard
          v-if="isViewer"
          data-testid="viewer-team"
        >
          <template #header>
            <h3 class="font-bold">
              Team
            </h3>
          </template>
          <p class="text-sm text-muted">
            See who is in the vault, and each member's Nostr profile.
          </p>
          <UButton
            to="/team"
            class="mt-3"
            icon="i-lucide-users"
          >
            View the team
          </UButton>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
