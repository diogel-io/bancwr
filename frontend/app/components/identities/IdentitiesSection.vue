<script setup lang="ts">
// The identities this bunker signs as (#103), on the Config page: each with its npub, which is
// the primary, and whether it is loaded. Administrators rename them here, create one whose key the
// bunker generates, and retire one (#146). A key never passes through the browser: creating sends
// only a label, and the response holds only the public key. The Config page is administrators'
// only, so signers see none of this.
import type { BunkerIdentity } from '#shared/types/bunker'

const { identities, list } = useIdentities()
const editing = ref<string>()
const draft = ref('')
const error = ref<string>()

const newLabel = ref('')
const creating = ref(false)
const created = ref<string>()

const retiring = ref<string>()
const retired = ref<string>()

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

/** The bunker's `error` code from a refused request, which the proxy passes on under `data`. */
function errorCode(failure: unknown): string | undefined {
  return (failure as { data?: { data?: { error?: string } } })?.data?.data?.error
}

async function create() {
  const label = newLabel.value.trim()
  if (!label) return
  error.value = undefined
  created.value = undefined
  retired.value = undefined
  creating.value = true
  try {
    const identity = await $fetch<BunkerIdentity>('/api/bunker/identities', { method: 'POST', body: { label } })
    newLabel.value = ''
    created.value = identity.label
    await identities.refresh()
  } catch (failure) {
    error.value = errorCode(failure) === 'root_key_unavailable'
      ? 'The identity was not created: the bunker cannot read its root credential (BANCWR_ROOT_KEY_FILE). Nothing was changed.'
      : 'The identity was not created: give it a label of up to 100 characters.'
  } finally {
    creating.value = false
  }
}

/** What retiring `identity` revokes, in words. */
function revokes(identity: BunkerIdentity): string {
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`
  const connections = count(identity.usage?.connections ?? 0, 'connected app', 'connected apps')
  const tokens = count(identity.usage?.tokens ?? 0, 'unused connection token', 'unused connection tokens')
  return `${connections} and ${tokens}`
}

async function retire(identity: BunkerIdentity) {
  error.value = undefined
  created.value = undefined
  try {
    await $fetch(`/api/bunker/identities/${encodeURIComponent(identity.pubkey)}`, { method: 'DELETE' })
    retiring.value = undefined
    retired.value = identity.label
    await identities.refresh()
  } catch {
    error.value = `${identity.label} could not be retired. Reload the page and try again.`
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
    <UAlert
      v-if="created"
      color="success"
      variant="subtle"
      :title="`Created ${created}. Grant it to members on their team page, and take a backup now: its key exists only in this bunker and its backups.`"
      class="mb-3"
      data-testid="identity-created"
    />
    <UAlert
      v-if="retired"
      color="neutral"
      variant="subtle"
      :title="`Retired ${retired}. Its key is destroyed, and its signatures stay in the log.`"
      class="mb-3"
      data-testid="identity-retired"
    />
    <ul class="divide-y divide-default">
      <li
        v-for="identity in list"
        :key="identity.pubkey"
        class="flex flex-col gap-2 py-3"
        :data-identity="identity.pubkey"
      >
        <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
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
          <div
            v-if="editing !== identity.pubkey && retiring !== identity.pubkey"
            class="flex shrink-0 gap-1"
          >
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              icon="i-lucide-pencil"
              :aria-label="`Rename ${identity.label}`"
              @click="edit(identity)"
            >
              Rename
            </UButton>
            <UButton
              v-if="!identity.is_primary"
              size="sm"
              color="neutral"
              variant="ghost"
              icon="i-lucide-archive"
              :aria-label="`Retire ${identity.label}`"
              data-testid="identity-retire"
              @click="retiring = identity.pubkey"
            >
              Retire
            </UButton>
          </div>
        </div>
        <div
          v-if="retiring === identity.pubkey"
          class="rounded-md border border-default p-3 text-sm"
          data-testid="identity-retire-confirm"
        >
          <p>
            Retire {{ identity.label }}? This revokes {{ revokes(identity) }}, removes every member's
            access to it, and destroys its key. Nothing can sign as it again unless you restore a
            backup taken before now. Its signatures stay in the log.
          </p>
          <div class="mt-2 flex gap-2">
            <UButton
              size="sm"
              color="error"
              @click="retire(identity)"
            >
              Retire {{ identity.label }}
            </UButton>
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              @click="retiring = undefined"
            >
              Cancel
            </UButton>
          </div>
        </div>
      </li>
    </ul>

    <form
      class="mt-4 flex flex-col gap-2 border-t border-default pt-4"
      data-testid="identity-create"
      @submit.prevent="create"
    >
      <label
        for="identity-create-label"
        class="text-sm font-medium"
      >
        Create an identity
      </label>
      <p class="text-xs text-muted">
        The bunker generates a new key and encrypts it under its root credential. The key is never
        shown and can't be exported, so if this bunker's database and every backup are lost, the
        identity is gone for good. Take a backup after creating one.
      </p>
      <div class="flex gap-2">
        <UInput
          id="identity-create-label"
          v-model="newLabel"
          maxlength="100"
          placeholder="Sales"
          class="w-full"
        />
        <UButton
          type="submit"
          size="sm"
          :loading="creating"
          :disabled="!newLabel.trim()"
        >
          Create identity
        </UButton>
      </div>
    </form>

    <template #footer>
      <p class="text-xs text-muted">
        To use a key you already have, run <code>bunker import-key --add</code> on the host, then restart the bunker; see
        "More than one identity" in <code>docs/operating.md</code>. Grant members an identity on their team page.
      </p>
    </template>
  </UCard>
</template>
