<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'
import { canOpen } from '~/utils/access'

defineProps<{ collapsed?: boolean }>()

// Flat, because Bancwr has ten screens. The template this is modelled on nests a Settings
// section, but it has sixteen.
const allLinks: (NavigationMenuItem & { to: string })[] = [
  { label: 'Dashboard', icon: 'i-lucide-layout-dashboard', to: '/' },
  { label: 'Config', icon: 'i-lucide-settings', to: '/config' },
  { label: 'Team', icon: 'i-lucide-users', to: '/team' },
  // Departments (#147): administrators, and the leads of a department.
  { label: 'Departments', icon: 'i-lucide-building-2', to: '/departments' },
  { label: 'Logs', icon: 'i-lucide-scroll-text', to: '/logs' },
  // Administrators' and signers' own profile (#30), follow list (#32) and relay list (#33) (#77).
  { label: 'Profile', icon: 'i-lucide-user', to: '/profile' },
  { label: 'Follows', icon: 'i-lucide-users-round', to: '/follows' },
  { label: 'Relays', icon: 'i-lucide-radio-tower', to: '/relays' },
  // Apps connected over NIP-46 for the signer; all of them for administrators (#31).
  { label: 'Connections', icon: 'i-lucide-plug', to: '/connections' },
  // Requests held for an administrator's approval before signing (#102).
  { label: 'Approvals', icon: 'i-lucide-shield-check', to: '/approvals' }
]

// Only what the signed-in role may open (#26, #77, utils/access.ts). The middleware refuses the rest
// anyway, so offering it would only lead to a permission-denied page.
const auth = useAuth()
const route = useRoute()
const { isLead } = useIdentities()
const links = computed<NavigationMenuItem[]>(() => {
  const state = auth.state.value
  if (state.status !== 'signed-in') return []
  const allowed = allLinks.filter(link => canOpen(state.role, link.to, isLead.value))
  // An identity's profile, follows and relays (#150) have no link of their own. They are reached
  // from Config, or for a lead from Departments (#147), which is marked active, so nobody takes the
  // page for their own.
  const from = state.role === 'administrator' ? '/config' : '/departments'
  return route.path.startsWith('/identities/') ? allowed.map(link => ({ ...link, active: link.to === from })) : allowed
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
