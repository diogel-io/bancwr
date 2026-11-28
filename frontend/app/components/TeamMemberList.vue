<script setup lang="ts">
// The vault's members, for administrators (#143): name (linking to their read-only profile, #77),
// npub and role. Each role is a select that changes it in one step (#99), and each row has a remove
// button. A department lead's role reads "Lead of Sales" beside it (#147).
import { useToast, ref } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import { ROLE_LABELS, ROLES, isRole } from '#shared/types/bunker'
import type { BunkerIdentity, ChangeRoleRequest, Role, TeamMember } from '#shared/types/bunker'
import { leadOf } from '~/utils/departments'

const props = defineProps<{
  data: TeamMember[]
  /** The bunker's identities, with their leads (#147). */
  identities?: BunkerIdentity[]
}>()

const leads = (member: TeamMember) => leadOf(props.identities ?? [], member.pubkey)

const emit = defineEmits<{
  refresh: []
}>()

const toast = useToast()
const auth = useAuth()

const columns: TableColumn<TeamMember>[] = [
  { id: 'name', header: 'Name' },
  { id: 'npub', header: 'Nostr Pubkey' },
  { id: 'role', header: 'Role' },
  { id: 'actions', header: 'Actions' }
]

/** The member's profile page, by npub. None for a row from before #24 whose key is not valid. */
const profileLink = (member: TeamMember) => member.npub ? `/team/${member.npub}` : undefined

// The two roles (#77, #143), in order of access: Admin, Signer.
const roles = ROLES.map(value => ({ label: ROLE_LABELS[value], value }))

// Bumped when a change does not happen, so the select is redrawn with the stored role.
const revision = ref(0)

// The bunker applies the change, records who changed it, and refuses to demote the last
// administrator (409). Both roles sign, so the member's connections stay (#143).
const changeRole = async (member: TeamMember, role: Role) => {
  if (role === member.role) {
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
      <!-- A value outside the two roles can only be from before #24; shown as stored. -->
      <template #role-cell="{ row }">
        <div class="flex flex-wrap items-center gap-2">
        <USelect
          v-if="isRole(row.original.role)"
          :key="`${row.original.id}-${row.original.role}-${revision}`"
          :model-value="row.original.role"
          :items="roles"
          :aria-label="`Role of ${row.original.name}`"
          class="w-32"
          @update:model-value="changeRole(row.original, $event as Role)"
        />
        <template v-else>
          {{ row.original.role }}
        </template>
        <UBadge
          v-if="leads(row.original)"
          color="primary"
          variant="subtle"
          :data-lead-of="row.original.pubkey"
        >
          {{ leads(row.original) }}
        </UBadge>
        </div>
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
