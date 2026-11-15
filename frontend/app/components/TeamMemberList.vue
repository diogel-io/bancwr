<script setup lang="ts">
// The vault's members: name (linking to their read-only profile, #77), npub and role. For
// administrators each role is a select that changes it in one step (#99), and each row has a remove
// button. `readOnly` (a viewer's list, #77) shows the role as text and leaves the actions out.
import { useToast, computed, ref } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import { ROLE_LABELS, ROLES, canSign } from '#shared/types/bunker'
import type { ChangeRoleRequest, Role, TeamMember } from '#shared/types/bunker'

const props = defineProps<{
  data: TeamMember[]
  readOnly?: boolean
}>()

const emit = defineEmits<{
  refresh: []
}>()

const toast = useToast()
const auth = useAuth()

const columns = computed<TableColumn<TeamMember>[]>(() => [
  { id: 'name', header: 'Name' },
  { id: 'npub', header: 'Nostr Pubkey' },
  { id: 'role', header: 'Role' },
  ...(props.readOnly ? [] : [{ id: 'actions', header: 'Actions' }])
])

/** The member's profile page, by npub. None for a row from before #24 whose key is not valid. */
const profileLink = (member: TeamMember) => member.npub ? `/team/${member.npub}` : undefined

// The three roles of #77, in order of access: Admin, Signer, Viewer.
const roles = ROLES.map(value => ({ label: ROLE_LABELS[value], value }))

/** A role the select can show: a value from before #24 is not one, and is shown as stored. */
const isRole = (role: string): role is Role => (ROLES as readonly string[]).includes(role)

// Bumped when a change does not happen, so the select is redrawn with the stored role.
const revision = ref(0)

/** True when `confirm` agrees, or nothing needs confirming. */
const confirmed = (member: TeamMember, role: Role) =>
  !(role === 'viewer' && canSign(member.role))
  || confirm(`Make ${member.name} a viewer? Viewers cannot sign, so the apps connected for them will be disconnected.`)

// The bunker applies the change, revokes a new viewer's connections, records who changed it, and
// refuses to demote the last administrator (409).
const changeRole = async (member: TeamMember, role: Role) => {
  if (role === member.role) {
    return
  }
  if (!confirmed(member, role)) {
    revision.value++
    return
  }

  try {
    const body: ChangeRoleRequest = { role }
    await $fetch(`/api/bunker/team/${member.id}`, { method: 'PATCH', body })
    toast.add({ title: `${member.name} is now ${ROLE_LABELS[role]}`, color: 'success' })
    // Their own role: re-read it, so the console stops offering what the bunker now refuses.
    if (auth.state.value.status === 'signed-in' && auth.state.value.pubkey === member.pubkey) {
      await auth.refresh()
    }
    emit('refresh')
  } catch (e) {
    const status = (e as { statusCode?: number }).statusCode
    toast.add({
      title: status === 409 ? 'The last administrator cannot be demoted' : 'Failed to change role',
      color: 'error'
    })
    revision.value++
  }
}

// Takes the member's id (row.original.id), not the table row's own id, which is its index.
const removeMember = async (member: TeamMember) => {
  if (!confirm(`Are you sure you want to remove ${member.name}?`)) {
    return
  }

  try {
    await $fetch(`/api/bunker/team/${member.id}`, { method: 'DELETE' })
    toast.add({ title: 'Member removed', color: 'success' })
    emit('refresh')
  } catch (e) {
    toast.add({ title: 'Failed to remove member', color: 'error' })
  }
}
</script>

<template>
  <div class="space-y-4">
    <UTable
      :data="data"
      :columns="columns"
    >
      <template #name-cell="{ row }">
        <ULink
          v-if="profileLink(row.original)"
          :to="profileLink(row.original)"
          class="font-medium hover:underline"
          :aria-label="`View ${row.original.name}'s profile`"
        >
          {{ row.original.name }}
        </ULink>
        <span v-else>{{ row.original.name }}</span>
      </template>
      <!-- The npub, or the stored value for a row from before #24 whose key is not valid. -->
      <template #npub-cell="{ row }">
        <span class="font-mono text-xs break-all">{{ row.original.npub ?? row.original.pubkey }}</span>
      </template>
      <!-- A value outside the three roles can only be from before #24; shown as stored. -->
      <template #role-cell="{ row }">
        <USelect
          v-if="!readOnly && isRole(row.original.role)"
          :key="`${row.original.id}-${row.original.role}-${revision}`"
          :model-value="row.original.role"
          :items="roles"
          :aria-label="`Role of ${row.original.name}`"
          class="w-32"
          @update:model-value="changeRole(row.original, $event as Role)"
        />
        <template v-else>
          {{ ROLE_LABELS[row.original.role as Role] ?? row.original.role }}
        </template>
      </template>
      <template #actions-cell="{ row }">
        <UButton
          color="error"
          variant="ghost"
          icon="i-heroicons-trash"
          :aria-label="`Remove ${row.original.name}`"
          @click="removeMember(row.original)"
        />
      </template>
    </UTable>
  </div>
</template>
