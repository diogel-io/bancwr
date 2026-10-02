<script setup lang="ts">
// The signed-in member's follow list (#32), for administrators and users (#26). Read from and
// published to their relays, signed by their own signer; the bunker is not involved. Changes are
// staged and published together with Save changes.
import { npubEncode } from 'nostr-tools/nip19'
import { FollowsError } from '~/composables/useFollows'
import { SignerCancelled } from '~/composables/useSignerPrompt'
import { SignerMismatch } from '~/composables/useUserSigner'
import { displayName, type Follow } from '~/utils/follows'
import type { PublishResult } from '~/utils/relay-io'

const PAGE_SIZE = 50

const list = useFollows()
const { state, follows, exists, pendingAdds, pendingRemoves, dirty, profiles, pubkey, searched } = list
const { reconnecting, withSigner, reconnected } = useSignerPrompt()
const nip46 = useNip46()

const npub = computed(() => pubkey.value ? npubEncode(pubkey.value) : '')
const filter = ref('')
const page = ref(1)

/** Followed keys, then staged adds, each with its staged change. */
const rows = computed(() => [
  ...follows.value.map(follow => ({ follow, pending: pendingRemoves.value.includes(follow.pubkey) ? 'remove' as const : undefined })),
  ...pendingAdds.value.map(pubkey => ({ follow: { pubkey, tag: ['p', pubkey] } as Follow, pending: 'add' as const }))
])
const filtered = computed(() => {
  const query = filter.value.trim().toLowerCase()
  if (!query) return rows.value
  return rows.value.filter(({ follow }) => {
    const profile = profiles.value[follow.pubkey]
    return [displayName(follow, profile), npubEncode(follow.pubkey), follow.pubkey, profile?.nip05 ?? '']
      .some(text => text.toLowerCase().includes(query))
  })
})
const shown = computed(() => filtered.value.slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE))
watch(filter, () => (page.value = 1))

const staged = computed(() => new Set([...follows.value.map(f => f.pubkey), ...pendingAdds.value]))

/**
 * Starting a follow list when none was found needs the member to say so (#62's flow): their list
 * may be on a relay not searched, and a new one would replace it everywhere.
 */
const confirmNew = ref(false)
watch(exists, () => (confirmNew.value = false))
const canSave = computed(() => dirty.value && (exists.value || confirmNew.value))

const saving = ref(false)
const saveError = ref<string>()
const published = ref<PublishResult>()

onMounted(() => list.load())

async function save() {
  if (!canSave.value || saving.value) return
  saveError.value = undefined
  published.value = undefined
  await withSigner(async (signer) => {
    saving.value = true
    try {
      published.value = await list.save(signer)
    } catch (failure) {
      saveError.value = failure instanceof FollowsError || failure instanceof SignerMismatch
        ? failure.message
        : 'Your signer did not sign the update, so nothing was saved. Try again, and approve it.'
    } finally {
      saving.value = false
    }
  }).catch((failure) => {
    if (failure instanceof SignerCancelled) saveError.value = 'Not saved: no signer was connected.'
  })
}

function add(key: string) {
  list.stageAdd(key)
  published.value = undefined
}

function undo(key: string, pending: 'add' | 'remove') {
  if (pending === 'add') list.stageRemove(key)
  else list.stageAdd(key)
}

// Staged changes are not lost silently.
onBeforeRouteLeave(() => {
  if (dirty.value && !saving.value && !window.confirm('Leave without saving your follow list changes?')) return false
})
function beforeUnload(event: BeforeUnloadEvent) {
  if (dirty.value) event.preventDefault()
}
onMounted(() => window.addEventListener('beforeunload', beforeUnload))
onBeforeUnmount(() => window.removeEventListener('beforeunload', beforeUnload))
</script>

<template>
  <UDashboardPanel id="follows">
    <template #header>
      <AppNavbar title="Follows" />
    </template>

    <template #body>
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
        Reading your follow list from your relays…
      </div>

      <UAlert
        v-else-if="state === 'failed'"
        color="error"
        variant="subtle"
        title="Couldn't reach any of your relays"
        description="Your follow list could not be read, so it can't be changed safely right now."
        :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => list.load() }]"
        data-testid="follows-load-failed"
      />

      <UCard
        v-else
        class="max-w-3xl"
      >
        <template #header>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h2 class="font-semibold">
              Following
              <span
                class="text-muted font-normal"
                data-testid="follows-count"
              >{{ follows.length }}</span>
            </h2>
            <UInput
              v-if="rows.length > 0"
              v-model="filter"
              icon="i-lucide-search"
              placeholder="Filter by name, npub or NIP-05"
              class="w-full sm:w-72"
              aria-label="Filter follows"
            />
          </div>
        </template>

        <div class="space-y-6">
          <NotFoundOnRelays
            v-if="!exists"
            noun="follow list"
            :searched="searched"
            :search="list.searchRelay"
            data-testid="follows-not-found"
          />

          <FollowsAddFollow
            :following="staged"
            :self="pubkey"
            :profiles="profiles"
            :load-profiles="list.loadProfiles"
            @add="add"
          />

          <p
            v-if="exists && rows.length === 0"
            class="text-sm text-muted"
            data-testid="follows-empty"
          >
            You follow no one yet.
          </p>

          <div v-if="rows.length > 0">
            <ul
              class="divide-y divide-default"
              data-testid="follows-list"
            >
              <FollowsFollowRow
                v-for="row in shown"
                :key="row.follow.pubkey"
                :follow="row.follow"
                :profile="profiles[row.follow.pubkey]"
                :pending="row.pending"
                @remove="list.stageRemove(row.follow.pubkey)"
                @undo="undo(row.follow.pubkey, row.pending!)"
              />
            </ul>
            <p
              v-if="filtered.length === 0"
              class="py-3 text-sm text-muted"
            >
              No follows match “{{ filter }}”.
            </p>
            <UPagination
              v-if="filtered.length > PAGE_SIZE"
              v-model:page="page"
              class="mt-3"
              :total="filtered.length"
              :items-per-page="PAGE_SIZE"
            />
          </div>
        </div>

        <template #footer>
          <div class="space-y-3">
            <UAlert
              v-if="saveError"
              color="error"
              variant="subtle"
              :title="saveError"
              data-testid="follows-save-error"
            />
            <UAlert
              v-if="published"
              color="success"
              variant="subtle"
              title="Follow list saved"
              data-testid="follows-saved"
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
              label="I have no follow list on another relay: start a new one."
            />
            <div class="flex flex-wrap items-center justify-end gap-3">
              <span
                v-if="dirty"
                class="text-xs text-muted"
                data-testid="follows-pending"
              >{{ pendingAdds.length }} to follow, {{ pendingRemoves.length }} to unfollow</span>
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
                data-testid="follows-save"
                @click="save"
              >
                {{ exists ? 'Save changes' : 'Start follow list' }}
              </UButton>
            </div>
          </div>
        </template>
      </UCard>

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
