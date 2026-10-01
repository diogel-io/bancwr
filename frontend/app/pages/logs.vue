<script setup lang="ts">
import { useFetch } from '#imports'
import type { LogEntry } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

const { data: logs, error } = await useFetch<LogEntry[]>('/api/bunker/logs', { key: 'logs-full' })
</script>

<template>
  <UDashboardPanel id="logs">
    <template #header>
      <UDashboardNavbar title="Signing Activity Log">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <UCard v-else>
        <ActivityLog :rows="logs ?? []" />
      </UCard>
    </template>
  </UDashboardPanel>
</template>
