<script setup lang="ts">
import { useToast } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { Role, TeamMember } from '#shared/types/bunker'

defineProps<{
  data: TeamMember[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const toast = useToast()

const columns: TableColumn<TeamMember>[] = [
  { accessorKey: 'name', header: 'Name' },
  { id: 'npub', header: 'Nostr Pubkey' },
  { id: 'role', header: 'Role' },
  { id: 'actions', header: 'Actions' }
]

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
