<script setup lang="ts">
// Departments (#147, departments ADR): each identity that isn't administrator only, with its
// members and leads. Administrators see every department and the administrator only identities,
// listed apart; a lead sees the departments they lead and manages them here. The bunker lists to
// each only what they may manage, and refuses anything else.
import type { BunkerIdentity, TeamMember } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'
import { bunkerRefusal } from '~/utils/bunker-error'
import { DEPARTMENT_ERRORS, departmentsOf } from '~/utils/departments'

const auth = useAuth()
const administrator = computed(() => auth.state.value.status === 'signed-in' && auth.state.value.role === 'administrator')
const self = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

const { identities, list } = useIdentities()
const { data: team, error, refresh: refreshTeam } = useFetch<TeamMember[]>('/api/bunker/team', { key: 'departments-team', default: () => [] })

const departments = computed(() => departmentsOf(list.value))
const adminOnly = computed(() => list.value.filter(identity => identity.admin_only))
// Grants decide who may use the primary once there is another identity or a department has had a
// lead (#103, #147), and from then on someone always holds one, until every grant is removed.
const grantsMatter = computed(() => list.value.length > 1 || list.value.some(identity => (identity.members ?? []).length > 0))

const actionError = ref<string>()
async function refresh() {
  await Promise.all([identities.refresh(), refreshTeam()])
}

async function open(identity: BunkerIdentity) {
  actionError.value = undefined
  try {
    await $fetch(`/api/bunker/identities/${encodeURIComponent(identity.pubkey)}`, { method: 'PATCH', body: { admin_only: false } })
    await refresh()
  } catch (failure) {
    const refusal = bunkerRefusal(failure)
    actionError.value = (refusal.error && DEPARTMENT_ERRORS[refusal.error]) ?? 'That change was not accepted. Try again.'
  }
}
</script>

<template>
  <UDashboardPanel id="departments">
    <template #header>
      <AppNavbar title="Departments" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <div
        v-else
        class="max-w-3xl space-y-6"
      >
        <UAlert
          color="neutral"
          variant="subtle"
          icon="i-lucide-info"
          :title="administrator ? 'Each identity is a department, used by its members.' : 'You lead these departments.'"
          description="Members may sign as their department's identity. Leads add and remove its members, issue its connections and approve its requests."
          data-testid="departments-explainer"
        />
        <UAlert
          v-if="actionError"
          color="error"
          variant="subtle"
          :title="actionError"
        />

        <DepartmentsDepartmentCard
          v-for="identity in departments"
          :key="identity.pubkey"
          :identity="identity"
          :team="team ?? []"
          :administrator="administrator"
          :self="self"
          :grants-matter="grantsMatter"
          @changed="refresh"
        />
        <p
          v-if="!departments.length"
          class="text-sm text-muted"
          data-testid="departments-empty"
        >
          {{ administrator ? 'Every identity is administrator only.' : 'You lead no departments.' }}
        </p>

        <UCard
          v-if="administrator && adminOnly.length"
          data-testid="admin-only-identities"
        >
          <template #header>
            <h2 class="font-semibold">
              Administrator only
            </h2>
            <p class="text-sm text-muted">
              These belong to no department: only administrators sign as them, manage their connections and approve their requests.
            </p>
          </template>
          <ul class="divide-y divide-default">
            <li
              v-for="identity in adminOnly"
              :key="identity.pubkey"
              class="flex flex-wrap items-center justify-between gap-2 py-2"
              :data-admin-only="identity.pubkey"
            >
              <span class="font-medium">{{ identity.label }}</span>
              <UButton
                size="xs"
                variant="ghost"
                data-testid="admin-only-open"
                @click="open(identity)"
              >
                Make it a department
              </UButton>
            </li>
          </ul>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
