<script setup lang="ts">
// Every page's header (#28): one navbar, so the health indicator is on all of them and they cannot
// drift apart. The indicator sits beside the sidebar control, top left.
defineProps<{ title: string }>()
</script>

<template>
  <UDashboardNavbar :title="title">
    <template #leading>
      <UDashboardSidebarCollapse />
      <BunkerHealth />
    </template>
    <template #right>
      <slot />
    </template>
  </UDashboardNavbar>
</template>
