<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'
import { canOpen } from '~/utils/access'

defineProps<{ collapsed?: boolean }>()

// Flat, because Bancwr has seven screens. The template this is modelled on nests a Settings
// section, but it has sixteen.
const allLinks: (NavigationMenuItem & { to: string })[] = [
  { label: 'Dashboard', icon: 'i-lucide-layout-dashboard', to: '/' },
  { label: 'Config', icon: 'i-lucide-settings', to: '/config' },
  { label: 'Team', icon: 'i-lucide-users', to: '/team' },
  { label: 'Logs', icon: 'i-lucide-scroll-text', to: '/logs' },
  // Every role's own profile (#30).
  { label: 'Profile', icon: 'i-lucide-user', to: '/profile' },
  // Administrators' and users' own follow list (#32).
  { label: 'Follows', icon: 'i-lucide-users-round', to: '/follows' },
  // Administrators' and users' own relay list (#33).
  { label: 'Relays', icon: 'i-lucide-radio-tower', to: '/relays' }
]

// Only what the signed-in role may open (#26): the middleware refuses the rest anyway, so offering
// it would only lead to a permission-denied page.
const auth = useAuth()
const links = computed<NavigationMenuItem[]>(() => {
  const state = auth.state.value
  return state.status === 'signed-in' ? allLinks.filter(link => canOpen(state.role, link.to)) : []
})
</script>

<template>
  <UNavigationMenu
    :collapsed="collapsed"
    :items="links"
    orientation="vertical"
    tooltip
  />
</template>
