<script setup lang="ts">
import { ref } from 'vue'

// Bound to UDashboardSidebar's overlay state on narrow viewports. The previous layout hid its nav
// behind `hidden md:flex`, so there was no mobile navigation at all.
const open = ref(false)
</script>

<template>
  <UDashboardGroup unit="rem">
    <UDashboardSidebar
      id="bancwr"
      v-model:open="open"
      collapsible
      resizable
      class="bg-[var(--bancwr-sidebar-bg)] border-e border-[var(--bancwr-sidebar-border)]"
      :ui="{ footer: 'lg:border-t lg:border-default' }"
    >
      <template #header="{ collapsed }">
        <NuxtLink
          to="/"
          class="flex items-center gap-2 font-bold text-xl"
        >
          <DiogelLogo size="sm" />
          <span v-if="!collapsed">Bancwr</span>
        </NuxtLink>
      </template>

      <template #default="{ collapsed }">
        <AppSidebar :collapsed="collapsed" />
      </template>

      <template #footer="{ collapsed }">
        <div
          class="flex items-center gap-2 w-full"
          :class="collapsed ? 'flex-col' : 'justify-between'"
        >
          <UColorModeSwitch />
          <UButton
            icon="i-simple-icons-github"
            color="neutral"
            variant="ghost"
            to="https://github.com/threenine/bancwr-diogel"
            target="_blank"
            aria-label="Source on GitHub"
          />
        </div>
      </template>
    </UDashboardSidebar>

    <slot />
  </UDashboardGroup>
</template>
