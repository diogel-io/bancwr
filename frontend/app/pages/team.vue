<script setup lang="ts">
import { useFetch, ref, reactive, useToast } from '#imports'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { AddTeamMemberRequest, Role, TeamMember } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

const { data: team, error, refresh } = await useFetch<TeamMember[]>('/api/bunker/team', { key: 'team-list' })

const state = reactive<AddTeamMemberRequest>({
  name: '',
  pubkey: '',
  role: 'signer'
})

// The three roles settled in #24, in order of access.
const roles = (['administrator', 'user', 'signer'] as Role[]).map(value => ({ label: ROLE_LABELS[value], value }))

const loading = ref(false)
const toast = useToast()

const addMember = async () => {
  loading.value = true
  try {
    await $fetch('/api/bunker/team', {
      method: 'POST',
      body: state
    })
    toast.add({ title: 'Member added successfully', color: 'success' })
    await refresh()
    // Reset form
    state.name = ''
    state.pubkey = ''
    state.role = 'signer'
  } catch (e) {
    toast.add({ title: 'Failed to add member', color: 'error' })
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <UDashboardPanel id="team">
    <template #header>
      <AppNavbar title="Team Management" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <div
        v-else
        class="space-y-6"
      >
        <TeamMemberList
          :data="team || []"
          @refresh="refresh"
        />

        <UCard>
          <template #header>
            <h3 class="text-base font-semibold leading-6">
              Add Team Member
            </h3>
          </template>

          <div class="space-y-4">
            <UFormField label="Name">
              <UInput
                v-model="state.name"
                placeholder="Alice"
              />
            </UFormField>
            <UFormField label="Pubkey">
              <UInput
                v-model="state.pubkey"
                placeholder="npub1… or hex"
              />
            </UFormField>
            <UFormField label="Role">
              <USelect
                v-model="state.role"
                :items="roles"
              />
            </UFormField>
          </div>

          <template #footer>
            <div class="flex justify-end gap-2">
              <UButton
                color="neutral"
                variant="ghost"
              >
                Cancel
              </UButton>
              <UButton
                :loading="loading"
                color="primary"
                @click="addMember"
              >
                Add Member
              </UButton>
            </div>
          </template>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
