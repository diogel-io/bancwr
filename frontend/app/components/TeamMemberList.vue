<script setup lang="ts">
import { useToast } from '#imports'
import type { TableColumn } from '@nuxt/ui'
import type { TeamMember } from '#shared/types/bunker'

defineProps<{
  data: TeamMember[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const toast = useToast()

const columns: TableColumn<TeamMember>[] = [
  { accessorKey: 'name', header: 'Name' },
  { accessorKey: 'pubkey', header: 'Nostr Pubkey' },
  { accessorKey: 'role', header: 'Role' },
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
