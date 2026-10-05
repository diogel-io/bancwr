<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'
import { canOpen } from '~/utils/access'

defineProps<{ collapsed?: boolean }>()

// Flat, because Bancwr has eight screens. The template this is modelled on nests a Settings
// section, but it has sixteen.
const allLinks: (NavigationMenuItem & { to: string })[] = [
  { label: 'Dashboard', icon: 'i-lucide-layout-dashboard', to: '/' },
  { label: 'Config', icon: 'i-lucide-settings', to: '/config' },
  { label: 'Team', icon: 'i-lucide-users', to: '/team' },
  { label: 'Logs', icon: 'i-lucide-scroll-text', to: '/logs' },
  // Administrators' and signers' own profile (#30), follow list (#32) and relay list (#33) (#77).
  { label: 'Profile', icon: 'i-lucide-user', to: '/profile' },
  { label: 'Follows', icon: 'i-lucide-users-round', to: '/follows' },
  { label: 'Relays', icon: 'i-lucide-radio-tower', to: '/relays' },
  // Apps connected over NIP-46 for the signer; all of them for administrators (#31).
  { label: 'Connections', icon: 'i-lucide-plug', to: '/connections' }
]

// Only what the signed-in role may open (#26, #77, utils/access.ts); a viewer gets the dashboard and
// Team. The middleware refuses the rest anyway, so offering
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
