<script setup lang="ts">
import { useFetch } from '#imports'

const { data: logs } = await useFetch('/api/bunker/logs', { key: 'logs-full' })
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
      <UCard>
        <ActivityLog :rows="logs" />
      </UCard>
    </template>
  </UDashboardPanel>
</template>
