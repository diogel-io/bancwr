<script setup lang="ts">
// A member's departments (#103, #147), on their team page, for administrators of a bunker with more
// than one identity: in each, not a member, a member or its lead. Taking someone out of a department
// disconnects their apps that sign as its identity; making a lead a member keeps them in it.
// Administrators may sign as every identity, so a department role only labels them. Administrator
// only identities have no members, and are listed apart.
import type { BunkerIdentity, DepartmentRole } from '#shared/types/bunker'
import { bunkerRefusal } from '~/utils/bunker-error'
import { DEPARTMENT_ERRORS, departmentsOf, roleIn } from '~/utils/departments'

const props = defineProps<{ member: string, administrator: boolean }>()
const { identities, list, several } = useIdentities()
const departments = computed(() => departmentsOf(list.value))
const adminOnly = computed(() => list.value.filter(identity => identity.admin_only))
const busy = ref<string>()
const error = ref<string>()
// Bumped when a change does not happen, so each select is redrawn with the stored role.
const revision = ref(0)

type Choice = DepartmentRole | 'none'
const CHOICES: { label: string, value: Choice }[] = [
  { label: 'Not in it', value: 'none' },
  { label: 'Member', value: 'member' },
  { label: 'Lead', value: 'lead' }
]

function current(identity: BunkerIdentity): Choice {
  return roleIn(identity, props.member) ?? 'none'
}

async function choose(identity: BunkerIdentity, choice: Choice) {
  if (choice === current(identity)) return
  busy.value = identity.pubkey
  error.value = undefined
  const path = `/api/bunker/identities/${encodeURIComponent(identity.pubkey)}/members/${encodeURIComponent(props.member)}`
  try {
    await $fetch(path, choice === 'none' ? { method: 'DELETE' } : { method: 'PUT', body: { role: choice } })
  } catch (failure) {
    const refusal = bunkerRefusal(failure)
    error.value = (refusal.error && DEPARTMENT_ERRORS[refusal.error]) ?? 'That change was not accepted. Try again.'
    revision.value++
  } finally {
    busy.value = undefined
    await identities.refresh()
  }
}
</script>

<template>
  <UCard
    v-if="several"
    data-testid="identity-access"
  >
    <template #header>
      <h2 class="font-semibold">
        Departments
      </h2>
      <p class="text-sm text-muted">
        {{ administrator ? 'Administrators may sign as every identity, so a department role only labels them.' : 'Taking them out of a department disconnects their apps for it.' }}
      </p>
    </template>
    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      class="mb-3"
    />
    <div class="space-y-2">
      <div
        v-for="identity in departments"
        :key="identity.pubkey"
        class="flex items-center justify-between gap-2"
        :data-identity="identity.pubkey"
      >
        <span>{{ identity.label }}</span>
        <USelect
          :id="`identity-access-${identity.pubkey}`"
          :key="`${identity.pubkey}-${current(identity)}-${revision}`"
          :model-value="current(identity)"
          :items="CHOICES"
          :disabled="busy === identity.pubkey"
          :aria-label="`Role in ${identity.label}`"
          class="w-36"
          @update:model-value="choice => choose(identity, choice as Choice)"
        />
      </div>
    </div>
    <p
      v-if="adminOnly.length"
      class="mt-3 text-xs text-muted"
      data-testid="identity-access-admin-only"
    >
      Administrator only, with no members: {{ adminOnly.map(identity => identity.label).join(', ') }}.
    </p>
  </UCard>
</template>
