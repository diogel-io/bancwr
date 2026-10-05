<script setup lang="ts">
// The vault's members (#24). Administrators add and remove them; viewers read the list (#77), with
// no add or remove controls, which the bunker would refuse them anyway (#25). Each row links to the
// member's read-only profile (team/[pubkey].vue).
import { useFetch, ref, reactive, useToast, computed } from '#imports'
import { ROLE_LABELS, ROLES } from '#shared/types/bunker'
import type { AddTeamMemberRequest, TeamMember } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

const auth = useAuth()
const isAdministrator = computed(() => auth.state.value.status === 'signed-in' && auth.state.value.role === 'administrator')

const { data: team, error, refresh } = await useFetch<TeamMember[]>('/api/bunker/team', { key: 'team-list' })

const state = reactive<AddTeamMemberRequest>({
  name: '',
  pubkey: '',
  role: 'signer'
})

// The three roles of #77, in order of access: Admin, Signer, Viewer.
const roles = ROLES.map(value => ({ label: ROLE_LABELS[value], value }))

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
      <AppNavbar :title="isAdministrator ? 'Team Management' : 'Team'" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <div
        v-else
        class="space-y-6"
      >
        <TeamMemberList
          :data="team || []"
          :read-only="!isAdministrator"
          @refresh="refresh"
        />

        <UCard
          v-if="isAdministrator"
          data-testid="add-member"
        >
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
