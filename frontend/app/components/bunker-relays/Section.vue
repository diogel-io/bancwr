<script setup lang="ts">
// The bunker's own relays (#78), on the Config page for administrators: the relays NIP-46 apps
// reach the bunker through, and that every bunker:// string carries. Not members' NIP-65 lists
// (/relays): these are stored in the bunker's database, never published to Nostr.
//
// NIP46_RELAYS, when set, decides: the list is then shown read-only. Otherwise an administrator
// adds relays by address or from a search of known relays, removes them, and saves the whole list,
// which the bunker applies at once. Changes are staged until saved.
import type { BunkerRelays, ReplaceBunkerRelaysRequest } from '#shared/types/bunker'
import { ADVISED_MIN_BUNKER_RELAYS, MAX_BUNKER_RELAYS } from '#shared/types/bunker'
import { isBunkerRelayUrl } from '~/utils/relay-discovery'
import { normalizeRelayUrl, relayKey } from '~/utils/relay-url'

const { data, error, refresh } = await useFetch<BunkerRelays>('/api/bunker/relays', { key: 'bunker-relays' })

const readOnly = computed(() => data.value?.source === 'environment')
const saved = computed(() => data.value?.relays ?? [])

/** Staged: saved relays to remove (by key), and relays to add, in the order added. */
const removed = ref(new Set<string>())
const added = ref<string[]>([])

const draft = computed(() => [
  ...saved.value.filter(relay => !removed.value.has(relayKey(relay.url))).map(relay => relay.url),
  ...added.value
])
const listed = computed(() => new Set(draft.value.map(relayKey)))
const removing = computed(() => saved.value.filter(relay => removed.value.has(relayKey(relay.url))).map(relay => relay.url))
const dirty = computed(() => removed.value.size > 0 || added.value.length > 0)
const full = computed(() => draft.value.length >= MAX_BUNKER_RELAYS)
/** NIP-46 on with nothing listed: no app could reach the bunker, so the bunker refuses it. */
const empty = computed(() => draft.value.length === 0 && !!data.value?.nip46_enabled)

const rows = computed(() => [
  ...saved.value.map(relay => ({
    url: relay.url,
    connected: relay.connected as boolean | undefined,
    pending: removed.value.has(relayKey(relay.url)) ? 'remove' as const : undefined
  })),
  ...added.value.map(url => ({ url, connected: undefined, pending: 'add' as const }))
])

const input = ref('')
const inputError = ref<string>()
watch(input, () => (inputError.value = undefined))

const saving = ref(false)
const saveError = ref<string>()
const savedNotice = ref(false)
/** Asked before saving a removal: apps using a removed relay must reconnect. */
const confirming = ref(false)

/** Any staged change: a confirmation or a saved notice no longer describes the list. */
function changed() {
  confirming.value = false
  savedNotice.value = false
}

function add(url: string): string | undefined {
  const result = normalizeRelayUrl(url)
  if ('error' in result) return result.error
  if (!isBunkerRelayUrl(result.url)) return 'Use wss://. Unencrypted ws:// is only allowed for a relay on this machine (localhost).'
  const key = relayKey(result.url)
  if (listed.value.has(key)) return 'This relay is already listed.'
  if (full.value) return `The bunker uses at most ${MAX_BUNKER_RELAYS} relays. Remove one first.`
  changed()
  // Removed and added back: just undo the removal, so it keeps its place.
  if (removed.value.has(key)) {
    const next = new Set(removed.value)
    next.delete(key)
    removed.value = next
  } else {
    added.value = [...added.value, result.url]
  }
  return undefined
}

function submitAddress() {
  inputError.value = add(input.value)
  if (!inputError.value) input.value = ''
}

function remove(url: string, pending: 'add' | 'remove' | undefined) {
  changed()
  if (pending === 'add') {
    added.value = added.value.filter(u => u !== url)
    return
  }
  removed.value = new Set([...removed.value, relayKey(url)])
}

function undo(url: string) {
  changed()
  const next = new Set(removed.value)
  next.delete(relayKey(url))
  removed.value = next
}

function discard() {
  confirming.value = false
  removed.value = new Set()
  added.value = []
  saveError.value = undefined
}

let recheck: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(recheck))

async function save() {
  if (!dirty.value || empty.value || saving.value) return
  if (removing.value.length > 0 && !confirming.value) {
    confirming.value = true
    return
  }
  saving.value = true
  saveError.value = undefined
  try {
    const body: ReplaceBunkerRelaysRequest = { relays: draft.value }
    data.value = await $fetch<BunkerRelays>('/api/bunker/relays', { method: 'PUT', body })
    discard()
    savedNotice.value = true
    // New relays take a moment to connect: look again shortly.
    clearTimeout(recheck)
    recheck = setTimeout(() => void refresh(), 3000)
  } catch (failure) {
    // The bunker's { error, message }, as the proxy forwards it: the body itself, or under the
    // proxy's own error's `data`.
    type Reply = { error?: string, message?: string, data?: { error?: string, message?: string } }
    const body = (failure as { data?: Reply }).data
    const reply = body?.data?.error ? body.data : body
    saveError.value = reply?.error === 'relays_from_environment'
      ? 'NIP46_RELAYS is now set on the bunker, so its relays cannot be changed here.'
      : reply?.message ?? 'The bunker did not save the relays. Try again.'
    if (reply?.error === 'relays_from_environment') await refresh()
  } finally {
    saving.value = false
    confirming.value = false
  }
}
</script>

<template>
  <UCard
    class="max-w-2xl"
    data-testid="bunker-relays-section"
  >
    <template #header>
      <h2 class="font-semibold">
        Bunker relays
      </h2>
      <p class="text-sm text-muted">
        The relays apps reach this bunker through over NIP-46, carried in every
        <code>bunker://</code> string. Kept in this bunker, never published to Nostr, and separate
        from members' own relay lists.
      </p>
    </template>

    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      title="Couldn't read the bunker's relays"
      :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => refresh() }]"
      data-testid="bunker-relays-error"
    />

    <div
      v-else-if="data"
      class="space-y-4"
    >
      <UAlert
        v-if="readOnly"
        color="neutral"
        variant="subtle"
        icon="i-lucide-lock"
        title="Set by NIP46_RELAYS"
        description="The bunker's NIP46_RELAYS setting decides these relays, so they can't be changed here. To manage them in the console, remove NIP46_RELAYS from the bunker's environment and restart it."
        data-testid="bunker-relays-read-only"
      />
      <UAlert
        v-else-if="!data.nip46_enabled"
        color="neutral"
        variant="subtle"
        icon="i-lucide-info"
        title="NIP-46 is turned off"
        description="The bunker uses these relays once NIP46_ENABLED=true turns NIP-46 on."
        data-testid="bunker-relays-nip46-off"
      />

      <ul
        v-if="rows.length > 0"
        class="divide-y divide-default"
        data-testid="bunker-relays-list"
      >
        <BunkerRelaysRelayRow
          v-for="row in rows"
          :key="row.url"
          :url="row.url"
          :connected="row.connected"
          :pending="row.pending"
          :read-only="readOnly"
          @remove="remove(row.url, row.pending)"
          @undo="undo(row.url)"
        />
      </ul>
      <p
        v-else
        class="text-sm text-muted"
        data-testid="bunker-relays-empty"
      >
        No relays are set, so no app can reach this bunker over NIP-46.
      </p>

      <UAlert
        v-if="empty"
        color="error"
        variant="subtle"
        icon="i-lucide-circle-alert"
        title="At least one relay is needed"
        description="NIP-46 is on: without a relay, no app can reach the bunker."
        data-testid="bunker-relays-none"
      />
      <UAlert
        v-else-if="draft.length < ADVISED_MIN_BUNKER_RELAYS"
        color="warning"
        variant="subtle"
        icon="i-lucide-triangle-alert"
        :title="`Fewer than ${ADVISED_MIN_BUNKER_RELAYS} relays`"
        description="If this relay is down, no app can reach the bunker. Two to six relays are advised."
        data-testid="bunker-relays-few"
      />

      <template v-if="!readOnly">
        <form
          class="flex flex-col gap-2 sm:flex-row sm:items-start"
          data-testid="bunker-relays-add"
          @submit.prevent="submitAddress"
        >
          <UFormField
            label="Add a relay"
            :help="full ? `The bunker uses at most ${MAX_BUNKER_RELAYS} relays.` : 'A wss:// address.'"
            class="flex-1"
            :error="inputError"
          >
            <UInput
              v-model="input"
              class="w-full"
              placeholder="wss://relay.example.com"
              autocomplete="off"
              :disabled="full"
            />
          </UFormField>
          <UButton
            type="submit"
            class="sm:mt-6"
            color="neutral"
            variant="outline"
            icon="i-lucide-plus"
            :disabled="full || !input.trim()"
          >
            Add
          </UButton>
        </form>

        <BunkerRelaysRelaySearch
          :listed="listed"
          :full="full"
          @add="url => add(url)"
        />
      </template>
    </div>

    <template
      v-if="data && !readOnly"
      #footer
    >
      <div class="space-y-3">
        <UAlert
          v-if="confirming"
          color="warning"
          variant="subtle"
          icon="i-lucide-triangle-alert"
          title="Apps may need to reconnect"
          :description="`Apps connected through ${removing.join(', ')} stop reaching the bunker until they reconnect with a new bunker:// string.`"
          data-testid="bunker-relays-confirm"
        />
        <UAlert
          v-if="saveError"
          color="error"
          variant="subtle"
          :title="saveError"
          data-testid="bunker-relays-save-error"
        />
        <UAlert
          v-if="savedNotice"
          color="success"
          variant="subtle"
          title="Relays saved"
          description="The bunker is using them now. New bunker:// strings carry this list."
          data-testid="bunker-relays-saved"
        />
        <div class="flex flex-wrap items-center justify-end gap-3">
          <span
            v-if="dirty"
            class="text-xs text-muted"
          >Unsaved changes</span>
          <UButton
            v-if="dirty"
            color="neutral"
            variant="ghost"
            :disabled="saving"
            @click="confirming ? (confirming = false) : discard()"
          >
            {{ confirming ? 'Keep editing' : 'Discard' }}
          </UButton>
          <UButton
            v-else
            color="neutral"
            variant="ghost"
            icon="i-lucide-refresh-cw"
            data-testid="bunker-relays-refresh"
            @click="refresh()"
          >
            Check connections
          </UButton>
          <UButton
            :loading="saving"
            :disabled="!dirty || empty"
            :color="confirming ? 'warning' : 'primary'"
            data-testid="bunker-relays-save"
            @click="save"
          >
            {{ confirming ? 'Save and remove' : 'Save relays' }}
          </UButton>
        </div>
      </div>
    </template>
  </UCard>
</template>
