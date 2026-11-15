<script setup lang="ts">
// The identities this bunker signs as (#103), on the Config page: each with its npub, which is
// the primary, and whether it is loaded. Administrators rename them here; adding one is done at
// the bunker's command line, so a key never passes through the browser.
import type { BunkerIdentity } from '#shared/types/bunker'

const { identities, list } = useIdentities()
const editing = ref<string>()
const draft = ref('')
const error = ref<string>()

function edit(identity: BunkerIdentity) {
  editing.value = identity.pubkey
  draft.value = identity.label
  error.value = undefined
}

async function save(identity: BunkerIdentity) {
  error.value = undefined
  try {
    await $fetch(`/api/bunker/identities/${encodeURIComponent(identity.pubkey)}`, { method: 'PATCH', body: { label: draft.value.trim() } })
    editing.value = undefined
    await identities.refresh()
  } catch {
    error.value = 'The label could not be saved: give it one of up to 100 characters.'
  }
}
</script>

<template>
  <UCard data-testid="identities-section">
    <template #header>
      <h2 class="font-semibold">
        Identities
      </h2>
      <p class="text-sm text-muted">
        The Nostr keys this bunker signs as. Each connection signs as one of them.
      </p>
    </template>

    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      class="mb-3"
      data-testid="identities-error"
    />
    <ul class="divide-y divide-default">
      <li
        v-for="identity in list"
        :key="identity.pubkey"
        class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center"
        :data-identity="identity.pubkey"
      >
        <div class="min-w-0 flex-1">
          <div
            v-if="editing === identity.pubkey"
            class="flex gap-2"
          >
            <UInput
              v-model="draft"
              maxlength="100"
              class="w-full"
              :aria-label="`Label for ${identity.label}`"
            />
            <UButton
              size="sm"
              @click="save(identity)"
            >
              Save
            </UButton>
          </div>
          <p
            v-else
            class="text-sm font-medium"
          >
            {{ identity.label }}
            <UBadge
              v-if="identity.is_primary"
              color="neutral"
              variant="subtle"
              size="sm"
              class="ml-1"
            >
              Primary
            </UBadge>
            <UBadge
              v-if="!identity.available"
              color="warning"
              variant="subtle"
              size="sm"
              class="ml-1"
              data-testid="identity-not-loaded"
            >
              Loads when the bunker restarts
            </UBadge>
          </p>
          <code class="text-xs text-muted break-all">{{ identity.npub ?? identity.pubkey }}</code>
        </div>
        <UButton
          v-if="editing !== identity.pubkey"
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-pencil"
          :aria-label="`Rename ${identity.label}`"
          @click="edit(identity)"
        >
          Rename
        </UButton>
      </li>
    </ul>
    <template #footer>
      <p class="text-xs text-muted">
        To add an identity, run <code>bunker import-key --add</code> on the host, then restart the bunker; see
        "More than one identity" in <code>docs/operating.md</code>. Grant members an identity on their team page.
      </p>
    </template>
  </UCard>
</template>
