<script setup lang="ts">
// The vault's members: name (linking to their read-only profile, #77), npub and role, and for
// administrators a remove button. `readOnly` (a viewer's list, #77) leaves the actions out.
import { useToast, computed } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { Role, TeamMember } from '#shared/types/bunker'

const props = defineProps<{
  data: TeamMember[]
  readOnly?: boolean
}>()

const emit = defineEmits<{
  refresh: []
}>()

const toast = useToast()

const columns = computed<TableColumn<TeamMember>[]>(() => [
  { id: 'name', header: 'Name' },
  { id: 'npub', header: 'Nostr Pubkey' },
  { id: 'role', header: 'Role' },
  ...(props.readOnly ? [] : [{ id: 'actions', header: 'Actions' }])
])

/** The member's profile page, by npub. None for a row from before #24 whose key is not valid. */
const profileLink = (member: TeamMember) => member.npub ? `/team/${member.npub}` : undefined

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
        {{ ROLE_LABELS[row.original.role as Role] ?? row.original.role }}
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
