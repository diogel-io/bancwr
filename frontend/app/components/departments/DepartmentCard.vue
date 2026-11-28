<script setup lang="ts">
// One department (#147): an identity, its members and its leads. Administrators and its leads add
// existing members and register new signers into it, and remove members; a lead may also step
// down or leave. Only administrators appoint or remove leads, and mark an identity with no members
// administrator only. Every change is the bunker's to allow, on every request.
import type { BunkerIdentity, DepartmentRole, TeamMember } from '#shared/types/bunker'
import { bunkerRefusal } from '~/utils/bunker-error'
import { DEPARTMENT_ERRORS, membersOf } from '~/utils/departments'

const props = defineProps<{
  identity: BunkerIdentity
  team: TeamMember[]
  /** Whether the signed-in member administers. */
  administrator: boolean
  /** The signed-in member's key. */
  self: string
  /** Whether grants decide who may use the primary yet: false on a bunker with one identity and no leads. */
  grantsMatter: boolean
}>()
const emit = defineEmits<{ changed: [] }>()

const members = computed(() => membersOf(props.identity, props.team))
const others = computed(() => props.team.filter(m => m.npub && !members.value.some(x => x.member.pubkey === m.pubkey)))
const addItems = computed(() => others.value.map(m => ({ label: m.name, value: m.pubkey })))
const chosen = ref<string>()
const chosenRole = ref<DepartmentRole>('member')
const newName = ref('')
const newKey = ref('')
const busy = ref(false)
const error = ref<string>()

const path = (member: string) => `/api/bunker/identities/${encodeURIComponent(props.identity.pubkey)}/members/${encodeURIComponent(member)}`

async function act(work: () => Promise<unknown>) {
  busy.value = true
  error.value = undefined
  try {
    await work()
    emit('changed')
  } catch (failure) {
    const refusal = bunkerRefusal(failure)
    error.value = (refusal.error && DEPARTMENT_ERRORS[refusal.error]) ?? refusal.message ?? 'That change was not accepted. Try again.'
  } finally {
    busy.value = false
  }
}

/** A lead removes members and themselves; another lead only an administrator. */
function mayRemove(member: string, role: DepartmentRole): boolean {
  return props.administrator || role === 'member' || member === props.self
}

const add = () => act(async () => {
  if (!chosen.value) return
  await $fetch(path(chosen.value), { method: 'PUT', body: { role: props.administrator ? chosenRole.value : 'member' } })
  chosen.value = undefined
  chosenRole.value = 'member'
})
const setRole = (member: string, role: DepartmentRole) => act(() => $fetch(path(member), { method: 'PUT', body: { role } }))
const remove = (member: TeamMember) => act(async () => {
  if (!confirm(`Remove ${member.name} from ${props.identity.label}? Their apps that sign as it are disconnected.`)) return
  await $fetch(path(member.pubkey), { method: 'DELETE' })
})
const register = () => act(async () => {
  await $fetch('/api/bunker/team', {
    method: 'POST',
    body: { name: newName.value.trim(), pubkey: newKey.value.trim(), role: 'signer', identities: [props.identity.pubkey] }
  })
  newName.value = ''
  newKey.value = ''
})
const markAdminOnly = () => act(() => $fetch(`/api/bunker/identities/${encodeURIComponent(props.identity.pubkey)}`, { method: 'PATCH', body: { admin_only: true } }))
</script>

<template>
  <UCard
    :data-department="identity.pubkey"
    data-testid="department"
  >
    <template #header>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 class="font-semibold">
            {{ identity.label }}
            <UBadge
              v-if="identity.is_primary"
              color="neutral"
              variant="subtle"
              class="ml-1"
            >
              Primary
            </UBadge>
          </h2>
          <p class="font-mono text-xs text-muted break-all">
            {{ identity.npub ?? identity.pubkey }}
          </p>
        </div>
        <UButton
          :to="`/identities/${identity.npub ?? identity.pubkey}/profile`"
          variant="outline"
          color="neutral"
          size="sm"
          icon="i-lucide-user-pen"
        >
          Edit profile
        </UButton>
      </div>
    </template>

    <div class="space-y-4">
      <UAlert
        v-if="error"
        color="error"
        variant="subtle"
        :title="error"
        data-testid="department-error"
      />
      <p
        v-if="!grantsMatter"
        class="text-sm text-muted"
        data-testid="department-everyone"
      >
        Everyone on the team may sign as {{ identity.label }} until another identity is added or a lead is appointed.
      </p>

      <ul
        v-if="members.length"
        class="divide-y divide-default"
        data-testid="department-members"
      >
        <li
          v-for="{ member, role } in members"
          :key="member.pubkey"
          class="flex flex-wrap items-center justify-between gap-2 py-2"
          :data-member="member.pubkey"
        >
          <div class="flex items-center gap-2">
            <span class="font-medium">{{ member.name }}{{ member.pubkey === self ? ' (you)' : '' }}</span>
            <UBadge
              v-if="role === 'lead'"
              color="primary"
              variant="subtle"
              data-testid="department-lead"
            >
              Lead
            </UBadge>
          </div>
          <div class="flex gap-1">
            <UButton
              v-if="administrator && role === 'member'"
              size="xs"
              variant="ghost"
              :disabled="busy"
              data-testid="department-appoint"
              @click="setRole(member.pubkey, 'lead')"
            >
              Make lead
            </UButton>
            <UButton
              v-if="role === 'lead' && (administrator || member.pubkey === self)"
              size="xs"
              variant="ghost"
              color="neutral"
              :disabled="busy"
              data-testid="department-step-down"
              @click="setRole(member.pubkey, 'member')"
            >
              {{ member.pubkey === self ? 'Step down as lead' : 'Remove as lead' }}
            </UButton>
            <UButton
              v-if="mayRemove(member.pubkey, role)"
              size="xs"
              variant="ghost"
              color="error"
              icon="i-lucide-user-minus"
              :disabled="busy"
              :aria-label="`Remove ${member.name} from ${identity.label}`"
              data-testid="department-remove"
              @click="remove(member)"
            />
          </div>
        </li>
      </ul>
      <p
        v-else-if="grantsMatter"
        class="text-sm text-muted"
        data-testid="department-empty"
      >
        Nobody is in {{ identity.label }} yet. Administrators may still sign as it.
      </p>

      <div
        v-if="addItems.length"
        class="flex flex-wrap items-end gap-2"
      >
        <UFormField
          label="Add a team member"
          class="min-w-48 flex-1"
        >
          <USelect
            v-model="chosen"
            :items="addItems"
            placeholder="Choose a member"
            class="w-full"
            :aria-label="`Add a team member to ${identity.label}`"
            data-testid="department-add-member"
          />
        </UFormField>
        <UFormField
          v-if="administrator"
          label="As"
        >
          <USelect
            v-model="chosenRole"
            :items="[{ label: 'Member', value: 'member' }, { label: 'Lead', value: 'lead' }]"
            class="w-28"
            :aria-label="`Role in ${identity.label}`"
          />
        </UFormField>
        <UButton
          :disabled="!chosen || busy"
          data-testid="department-add"
          @click="add"
        >
          Add
        </UButton>
      </div>

      <details class="text-sm">
        <summary class="cursor-pointer text-muted">
          Register someone new into {{ identity.label }}
        </summary>
        <div class="mt-2 flex flex-wrap items-end gap-2">
          <UFormField
            label="Name"
            class="min-w-40 flex-1"
          >
            <UInput
              v-model="newName"
              placeholder="Paul"
              class="w-full"
              :aria-label="`Name of the new member of ${identity.label}`"
            />
          </UFormField>
          <UFormField
            label="Pubkey"
            class="min-w-56 flex-1"
          >
            <UInput
              v-model="newKey"
              placeholder="npub1… or hex"
              class="w-full"
              :aria-label="`Pubkey of the new member of ${identity.label}`"
            />
          </UFormField>
          <UButton
            :disabled="!newName.trim() || !newKey.trim() || busy"
            data-testid="department-register"
            @click="register"
          >
            Register as a signer
          </UButton>
        </div>
      </details>

      <div
        v-if="administrator && !members.length && grantsMatter"
        class="flex justify-end"
      >
        <UButton
          variant="link"
          color="neutral"
          size="sm"
          :disabled="busy"
          data-testid="department-mark-admin-only"
          @click="markAdminOnly"
        >
          Make administrator only
        </UButton>
      </div>
    </div>
  </UCard>
</template>
