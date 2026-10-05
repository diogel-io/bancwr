<script setup lang="ts">
// The signed-in member's NIP-65 relay list (#33), for administrators and signers (#26, #77): where other
// apps find their posts (write) and mentions of them (read). Read from and published to their
// relays, signed by their own signer. Changes are staged and published together.
import { npubEncode } from 'nostr-tools/nip19'
import { RelayListError } from '~/composables/useRelayList'
import { SignerCancelled } from '~/composables/useSignerPrompt'
import { SignerMismatch } from '~/composables/useUserSigner'
import { relayKey } from '~/utils/relay-url'
import type { PublishResult } from '~/utils/relay-io'

/** NIP-65: "keep kind:10002 lists small (2-4 relays of each category)". */
const ADVISED_MAX = 4

const list = useRelayList()
const { state, entries, preview, exists, adds, removes, markers, dirty, answered, unanswered, pubkey, searched, defaults, indexers } = list
const { reconnecting, withSigner, reconnected } = useSignerPrompt()
const nip46 = useNip46()
const { health, ensure } = useBunkerHealth()

const npub = computed(() => pubkey.value ? npubEncode(pubkey.value) : '')
const host = (url: string) => url.replace(/^wss?:\/\//u, '').replace(/\/$/u, '')

/** Every relay shown: the list as it will be, plus the ones staged for removal, in list order. */
const rows = computed(() => {
  const removing = new Set(removes.value)
  const changed = new Set(Object.keys(markers.value))
  const added = new Set(adds.value.map(a => relayKey(a.url)))
  const upcoming = new Map(preview.value.map(e => [relayKey(e.url), e]))
  const listed = entries.value.map((entry) => {
    const key = relayKey(entry.url)
    if (removing.has(key)) return { ...entry, pending: 'remove' as const }
    const next = upcoming.get(key) ?? entry
    return { ...next, pending: changed.has(key) ? 'markers' as const : undefined }
  })
  const fresh = preview.value.filter(e => added.has(relayKey(e.url))).map(e => ({ ...e, pending: 'add' as const }))
  return [...listed, ...fresh].map(row => ({
    ...row,
    reachable: answered.value.has(relayKey(row.url)) ? true : unanswered.value.has(relayKey(row.url)) ? false : undefined
  }))
})
const listedKeys = computed(() => new Set([...entries.value.map(e => relayKey(e.url)), ...adds.value.map(a => relayKey(a.url))]))
const readCount = computed(() => preview.value.filter(e => e.read).length)
const writeCount = computed(() => preview.value.filter(e => e.write).length)

/** The relays this bunker reaches NIP-46 clients through (#27), as context only. */
const bunkerRelays = computed(() => health.value.checks.find(c => c.name === 'relays' && c.status !== 'disabled')?.relays ?? [])

/**
 * Starting a list when none was found needs the member to say so (#62's flow): their list may be
 * on a relay not searched, and a new one would replace it everywhere.
 */
const confirmNew = ref(false)
watch(exists, () => (confirmNew.value = false))
const canSave = computed(() => dirty.value && (exists.value || confirmNew.value))

const saving = ref(false)
const saveError = ref<string>()
const published = ref<PublishResult>()

onMounted(() => {
  list.load()
  void ensure()
})

function startFromDefaults() {
  for (const url of defaults) list.stageAdd(url.replace(/\/$/u, ''))
}

async function save() {
  if (!canSave.value || saving.value) return
  saveError.value = undefined
  published.value = undefined
  await withSigner(async (signer) => {
    saving.value = true
    try {
      published.value = await list.save(signer)
    } catch (failure) {
      saveError.value = failure instanceof RelayListError || failure instanceof SignerMismatch
        ? failure.message
        : 'Your signer did not sign the update, so nothing was saved. Try again, and approve it.'
    } finally {
      saving.value = false
    }
  }).catch((failure) => {
    if (failure instanceof SignerCancelled) saveError.value = 'Not saved: no signer was connected.'
  })
}

function undo(url: string, pending: 'add' | 'remove' | 'markers' | undefined) {
  if (pending === 'remove') list.stageAdd(url)
}

// Staged changes are not lost silently.
onBeforeRouteLeave(() => {
  if (dirty.value && !saving.value && !window.confirm('Leave without saving your relay list changes?')) return false
})
function beforeUnload(event: BeforeUnloadEvent) {
  if (dirty.value) event.preventDefault()
}
onMounted(() => window.addEventListener('beforeunload', beforeUnload))
onBeforeUnmount(() => window.removeEventListener('beforeunload', beforeUnload))
</script>

<template>
  <UDashboardPanel id="relays">
    <template #header>
      <AppNavbar title="Relays" />
    </template>

    <template #body>
      <SignedInAs class="mb-4" />
      <div
        v-if="state === 'idle' || state === 'loading'"
        class="flex items-center gap-2 text-sm text-muted"
        role="status"
      >
        <UIcon
          name="i-lucide-loader-circle"
          class="size-4 animate-spin"
          aria-hidden="true"
        />
        Reading your relay list…
      </div>

      <UAlert
        v-else-if="state === 'failed'"
        color="error"
        variant="subtle"
        title="Couldn't reach any of your relays"
        description="Your relay list could not be read, so it can't be changed safely right now."
        :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => list.load() }]"
        data-testid="relays-load-failed"
      />

      <div
        v-else
        class="max-w-3xl space-y-6"
      >
        <UCard>
          <template #header>
            <h2 class="font-semibold">
              Your relays
            </h2>
            <p class="text-sm text-muted">
              Other apps read your posts from your <strong>write</strong> relays, and look for
              mentions of you on your <strong>read</strong> relays (NIP-65).
            </p>
          </template>

          <div class="space-y-6">
            <div
              v-if="!exists"
              class="space-y-3"
            >
              <NotFoundOnRelays
                noun="relay list"
                :searched="searched"
                :search="list.searchRelay"
                data-testid="relays-not-found"
              />
            </div>

            <div
              v-if="!exists || (entries.length === 0 && adds.length === 0)"
              class="space-y-2 rounded-md border border-default p-3 text-sm"
              data-testid="relays-default"
            >
              <p v-if="exists">
                Your relay list is empty.
              </p>
              <p>
                <template v-if="!exists">
                  You have no relay list, so other apps can't tell where you publish.
                </template>
                Bancwr reads and publishes your profile and follows through
                {{ defaults.map(host).join(', ') }}, and the indexers {{ indexers.map(host).join(', ') }}.
              </p>
              <UButton
                v-if="adds.length === 0"
                size="sm"
                color="neutral"
                variant="outline"
                data-testid="relays-start-defaults"
                @click="startFromDefaults"
              >
                Start from {{ defaults.map(host).join(', ') }}
              </UButton>
            </div>

            <ul
              v-if="rows.length > 0"
              class="divide-y divide-default"
              data-testid="relays-list"
            >
              <RelaysRelayRow
                v-for="row in rows"
                :key="row.url"
                :url="row.url"
                :read="row.read"
                :write="row.write"
                :pending="row.pending"
                :reachable="row.reachable"
                @markers="value => list.stageMarkers(row.url, value)"
                @remove="list.stageRemove(row.url)"
                @undo="undo(row.url, row.pending)"
              />
            </ul>

            <UAlert
              v-if="(exists || adds.length > 0) && preview.length > 0 && writeCount === 0"
              color="warning"
              variant="subtle"
              icon="i-lucide-triangle-alert"
              title="No write relays"
              description="Other apps won't know where to find your posts. Mark at least one relay as Write."
              data-testid="relays-no-write"
            />
            <UAlert
              v-if="readCount > ADVISED_MAX || writeCount > ADVISED_MAX"
              color="neutral"
              variant="subtle"
              icon="i-lucide-info"
              :title="`${readCount} read and ${writeCount} write relays`"
              :description="`NIP-65 advises 2 to ${ADVISED_MAX} of each, so other apps do not have to connect to many relays to find you.`"
              data-testid="relays-too-many"
            />

            <RelaysAddRelay
              :listed="listedKeys"
              @add="url => list.stageAdd(url)"
            />
          </div>

          <template #footer>
            <div class="space-y-3">
              <UAlert
                v-if="saveError"
                color="error"
                variant="subtle"
                :title="saveError"
                data-testid="relays-save-error"
              />
              <UAlert
                v-if="published"
                color="success"
                variant="subtle"
                title="Relay list saved"
                data-testid="relays-saved"
              >
                <template #description>
                  <p>Accepted by {{ published.accepted.join(', ') }}.</p>
                  <p v-if="published.failed.length">
                    Not saved on {{ published.failed.map(f => f.url).join(', ') }}.
                  </p>
                </template>
              </UAlert>
              <div
                v-if="saving && nip46.phase.value !== 'idle'"
                role="status"
                class="text-sm"
              >
                Waiting for your signer to sign…
                <UButton
                  v-if="nip46.approvalUrl.value"
                  :to="nip46.approvalUrl.value"
                  target="_blank"
                  rel="noopener noreferrer"
                  variant="link"
                >
                  Approve in your signer
                </UButton>
              </div>
              <UCheckbox
                v-if="!exists"
                v-model="confirmNew"
                label="I have no relay list on another relay: start a new one."
                data-testid="relays-confirm-new"
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
                  @click="list.discard()"
                >
                  Discard
                </UButton>
                <UButton
                  :loading="saving"
                  :disabled="!canSave"
                  data-testid="relays-save"
                  @click="save"
                >
                  {{ exists ? 'Save changes' : 'Start relay list' }}
                </UButton>
              </div>
            </div>
          </template>
        </UCard>

        <UAlert
          v-if="bunkerRelays.length > 0"
          color="neutral"
          variant="subtle"
          icon="i-lucide-server"
          title="This bunker's relays"
          data-testid="bunker-relays"
        >
          <template #description>
            <p>
              This bunker reaches NIP-46 clients through {{ bunkerRelays.map(r => host(r.url)).join(', ') }}.
              An administrator sets these under Config (or with <code>NIP46_RELAYS</code>), separate
              from your list: the bunker signs as itself, never as you.
            </p>
          </template>
        </UAlert>
      </div>

      <UModal
        v-model:open="reconnecting"
        title="Connect your signer"
        :description="`Signing needs the key you signed in with (${npub}). Connect the signer that holds it.`"
      >
        <template #body>
          <SignerConnect
            :use="reconnected"
            :expected-pubkey="pubkey"
            extension-label="Use extension"
            bunker-label="Connect"
          />
        </template>
      </UModal>
    </template>
  </UDashboardPanel>
</template>
