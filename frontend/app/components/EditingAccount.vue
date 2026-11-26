<script setup lang="ts">
// Whose account the Profile, Follows and Relays pages edit (#70, #150): the signed-in member's
// own ("Signed in as"), or one of the bunker's identities, named in a banner so nobody mistakes it
// for their own. An identity the bunker does not have is said so, and nothing is edited.
import { npubEncode } from 'nostr-tools/nip19'
import type { EditTarget } from '~/composables/useEditTarget'

// The alerts are shrink-0: the page body is a flex column, which would otherwise squeeze them.
const props = defineProps<{ target: EditTarget }>()

const label = computed(() => props.target.identity.value?.label)
const npub = computed(() => props.target.pubkey ? npubEncode(props.target.pubkey) : '')
</script>

<template>
  <SignedInAs v-if="!target.editingIdentity" />
  <UAlert
    v-else-if="target.missing.value"
    class="shrink-0"
    color="error"
    variant="subtle"
    title="No such identity"
    description="This bunker has no identity with that key, so there is nothing to edit here."
    :actions="[{ label: 'Back to Config', color: 'neutral', variant: 'outline', to: '/config' }]"
    data-testid="editing-identity-missing"
  />
  <UAlert
    v-else
    class="shrink-0"
    color="info"
    variant="subtle"
    icon="i-lucide-building-2"
    :title="label ? `Editing ${label}` : 'Editing an identity'"
    data-testid="editing-identity"
  >
    <template #description>
      <p>
        This is one of the bunker's identities, not your own account. The bunker signs each change
        as it, and the signing log records it under your name.
      </p>
      <p class="font-mono text-xs break-all">
        {{ npub }}
      </p>
    </template>
  </UAlert>
</template>
