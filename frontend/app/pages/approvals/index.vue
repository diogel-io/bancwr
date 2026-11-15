<script setup lang="ts">
// Requests held for approval before signing (#102): administrators decide, signers see their own.
</script>

<template>
  <UDashboardPanel id="approvals">
    <template #header>
      <AppNavbar title="Approvals" />
    </template>
    <template #body>
      <ApprovalsApprovalQueue />
    </template>
  </UDashboardPanel>
</template>
