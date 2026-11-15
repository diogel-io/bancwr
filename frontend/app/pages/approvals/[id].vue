<script setup lang="ts">
// Where a held request's NIP-46 auth challenge points (#102): the queue, with that request picked
// out, or a note that it is no longer waiting.
const route = useRoute()
const focus = computed(() => String(route.params.id ?? ''))
</script>

<template>
  <UDashboardPanel id="approval">
    <template #header>
      <AppNavbar title="Approvals" />
    </template>
    <template #body>
      <ApprovalsApprovalQueue :focus="focus" />
    </template>
  </UDashboardPanel>
</template>
