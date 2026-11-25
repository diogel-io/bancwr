<script setup lang="ts">
// The vault's members (#24), for administrators only (#143): they add and remove them and change
// their roles (#99). Each row links to the member's read-only profile (team/[pubkey].vue).
import { useFetch, ref, reactive, useToast, computed, watch } from '#imports'
import { ROLE_LABELS, ROLES } from '#shared/types/bunker'
import type { AddTeamMemberRequest, TeamMember } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

const { data: team, error, refresh } = await useFetch<TeamMember[]>('/api/bunker/team', { key: 'team-list' })

const state = reactive<AddTeamMemberRequest>({
  name: '',
  pubkey: '',
  role: 'signer'
})

// The two roles (#77, #143), in order of access: Admin, Signer.
const roles = ROLES.map(value => ({ label: ROLE_LABELS[value], value }))

// The identities a new member may sign as (#103), offered only on a bunker with several: the
// primary preselected, as the bunker grants it when none is named.
const { list: identities, several } = useIdentities()
const chosenIdentities = ref<string[]>([])
watch(identities, (now) => {
  if (!chosenIdentities.value.length) chosenIdentities.value = now.filter(i => i.is_primary).map(i => i.pubkey)
}, { immediate: true })
const offersIdentities = computed(() => several.value)

function toggleIdentity(pubkey: string, on: boolean | 'indeterminate') {
  const rest = chosenIdentities.value.filter(p => p !== pubkey)
  chosenIdentities.value = on === true ? [...rest, pubkey] : rest
}

const loading = ref(false)
const toast = useToast()

const addMember = async () => {
  loading.value = true
  try {
    await $fetch('/api/bunker/team', {
      method: 'POST',
      body: { ...state, ...(offersIdentities.value ? { identities: chosenIdentities.value } : {}) }
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

        <UCard data-testid="add-member">
          <template #header>
            <h3 class="text-base font-semibold leading-6">
              Add Team Member
            </h3>
          </template>

          <div class="space-y-4">
            <UFormField label="Name">
              <UInput
                id="team-member-name"
                v-model="state.name"
                placeholder="Alice"
              />
            </UFormField>
            <UFormField label="Pubkey">
              <UInput
                id="team-member-pubkey"
                v-model="state.pubkey"
                placeholder="npub1… or hex"
              />
            </UFormField>
            <UFormField label="Role">
              <USelect
                id="team-member-role"
                v-model="state.role"
                :items="roles"
              />
            </UFormField>
            <UFormField
              v-if="offersIdentities"
              label="May sign as"
              data-testid="team-member-identities"
            >
              <div class="space-y-1.5">
                <UCheckbox
                  v-for="identity in identities"
                  :id="`team-member-identity-${identity.pubkey}`"
                  :key="identity.pubkey"
                  :model-value="chosenIdentities.includes(identity.pubkey)"
                  :label="identity.label"
                  :data-identity="identity.pubkey"
                  @update:model-value="on => toggleIdentity(identity.pubkey, on)"
                />
              </div>
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
