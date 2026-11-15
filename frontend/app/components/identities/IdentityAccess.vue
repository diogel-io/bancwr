<script setup lang="ts">
// Which identities a member may sign as (#103), on their team page, for administrators of a bunker
// with more than one. Removing one revokes the member's connections and unused tokens for it.
// Administrators may sign as every identity, so theirs are shown as always allowed.
import type { BunkerIdentity } from '#shared/types/bunker'

const props = defineProps<{ member: string, administrator: boolean }>()
const { identities, list, several } = useIdentities()
const busy = ref<string>()
const error = ref<string>()

function granted(identity: BunkerIdentity): boolean {
  return props.administrator || (identity.members ?? []).includes(props.member)
}

async function toggle(identity: BunkerIdentity, on: boolean | 'indeterminate') {
  busy.value = identity.pubkey
  error.value = undefined
  const path = `/api/bunker/identities/${encodeURIComponent(identity.pubkey)}/members/${encodeURIComponent(props.member)}`
  try {
    await $fetch(path, { method: on === true ? 'PUT' : 'DELETE' })
  } catch {
    error.value = 'That change was not accepted. Try again.'
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
        May sign as
      </h2>
      <p class="text-sm text-muted">
        {{ administrator ? 'Administrators may sign as every identity.' : 'Removing an identity disconnects their apps for it.' }}
      </p>
    </template>
    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      class="mb-3"
    />
    <div class="space-y-1.5">
      <UCheckbox
        v-for="identity in list"
        :id="`identity-access-${identity.pubkey}`"
        :key="identity.pubkey"
        :model-value="granted(identity)"
        :label="identity.label"
        :disabled="administrator || busy === identity.pubkey"
        :data-identity="identity.pubkey"
        @update:model-value="on => toggle(identity, on)"
      />
    </div>
  </UCard>
</template>
