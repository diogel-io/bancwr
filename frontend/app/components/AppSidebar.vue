<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'

defineProps<{ collapsed?: boolean }>()

// Flat, because Bancwr has four screens. The template this is modelled on nests a Settings
// section, but it has sixteen.
const links: NavigationMenuItem[] = [
  { label: 'Dashboard', icon: 'i-lucide-layout-dashboard', to: '/' },
  { label: 'Config', icon: 'i-lucide-settings', to: '/config' },
  { label: 'Team', icon: 'i-lucide-users', to: '/team' },
  { label: 'Logs', icon: 'i-lucide-scroll-text', to: '/logs' }
]
</script>

<template>
  <UNavigationMenu
    :collapsed="collapsed"
    :items="links"
    orientation="vertical"
    tooltip
  />
</template>
