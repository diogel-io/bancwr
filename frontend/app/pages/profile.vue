<script setup lang="ts">
// The signed-in member's own Nostr profile (#30), for administrators and signers (#26, #77). Read from and published to
// their relays, signed by their own signer; the bunker is not involved.
import { npubEncode } from 'nostr-tools/nip19'
import { ProfileError } from '~/composables/useProfile'
import { SignerCancelled } from '~/composables/useSignerPrompt'
import { SignerMismatch } from '~/composables/useUserSigner'
import { BlossomError, uploadToBlossom } from '~/utils/blossom'
import { ImageError, prepareImage, type ImageKind } from '~/utils/image'
import { validateForm } from '~/utils/profile'
import type { PublishResult } from '~/utils/relay-io'

const profile = useProfile()
const { state, form, dirty, exists, pubkey, blossomServer, searched } = profile
const nip46 = useNip46()

const npub = computed(() => pubkey.value ? npubEncode(pubkey.value) : '')
const errors = computed(() => validateForm(form.value))
const valid = computed(() => Object.keys(errors.value).length === 0)

/**
 * Creating a profile when none was found needs the member to say so (#62): their profile may be on
 * a relay not searched, and a new one would replace it everywhere.
 */
const confirmNew = ref(false)
watch(exists, () => (confirmNew.value = false))
const canSave = computed(() => valid.value && dirty.value && (exists.value || confirmNew.value))

const saving = ref(false)
const saveError = ref<string>()
const published = ref<PublishResult>()

onMounted(() => profile.load())

const { reconnecting, withSigner, reconnected } = useSignerPrompt()

function message(failure: unknown, fallback: string): string {
  if (failure instanceof ProfileError || failure instanceof SignerMismatch || failure instanceof BlossomError || failure instanceof ImageError) {
    return failure.message
  }
  return fallback
}

async function save() {
  if (!canSave.value || saving.value) return
  saveError.value = undefined
  published.value = undefined
  await withSigner(async (signer) => {
    saving.value = true
    try {
      published.value = await profile.save(signer)
    } catch (failure) {
      saveError.value = message(failure, 'Your signer did not sign the update, so nothing was saved. Try again, and approve it.')
    } finally {
      saving.value = false
    }
  }).catch((failure) => {
    if (failure instanceof SignerCancelled) saveError.value = 'Not saved: no signer was connected.'
  })
}

/** Image upload: prepared here, signed by the member's signer, sent to their Blossom server. */
function upload(file: File, kind: ImageKind): Promise<string> {
  return new Promise((resolve, reject) => {
    withSigner(async (signer) => {
      try {
        const blob = await prepareImage(file, kind)
        resolve(await uploadToBlossom(blossomServer.value, blob, signer))
      } catch (failure) {
        reject(new Error(message(failure, 'The upload failed.')))
      }
    }).catch(failure => reject(new Error(failure instanceof SignerCancelled
      ? 'Not uploaded: no signer was connected.'
      : message(failure, 'The upload failed.'))))
  })
}

// Unsaved changes are not lost silently.
onBeforeRouteLeave(() => {
  if (dirty.value && !saving.value && !window.confirm('Leave without saving your profile changes?')) return false
})
function beforeUnload(event: BeforeUnloadEvent) {
  if (dirty.value) event.preventDefault()
}
onMounted(() => window.addEventListener('beforeunload', beforeUnload))
onBeforeUnmount(() => window.removeEventListener('beforeunload', beforeUnload))
</script>

<template>
  <UDashboardPanel id="profile">
    <template #header>
      <AppNavbar title="Profile" />
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
        Reading your profile from your relays…
      </div>

      <UAlert
        v-else-if="state === 'failed'"
        color="error"
        variant="subtle"
        title="Couldn't reach any of your relays"
        description="Your profile could not be read, so it can't be edited safely right now."
        :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => profile.load() }]"
        data-testid="profile-load-failed"
      />

      <div
        v-else
        class="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]"
        data-testid="profile-layout"
      >
        <UCard>
          <template #header>
            <h2 class="font-semibold">
              Profile Details
            </h2>
          </template>

          <NotFoundOnRelays
            v-if="!exists"
            class="mb-6"
            :searched="searched"
            :search="profile.searchRelay"
            data-testid="profile-not-found"
          />

          <ProfileForm
            v-model="form"
            :pubkey="pubkey"
            :errors="errors"
            :upload="upload"
          />

          <template #footer>
            <div class="space-y-3">
              <UAlert
                v-if="saveError"
                color="error"
                variant="subtle"
                :title="saveError"
                data-testid="profile-save-error"
              />
              <UAlert
                v-if="published"
                color="success"
                variant="subtle"
                title="Profile saved"
                data-testid="profile-saved"
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
                label="I have no profile on another relay: create a new one."
                data-testid="profile-confirm-new"
              />
              <div class="flex items-center justify-end gap-3">
                <span
                  v-if="dirty"
                  class="text-xs text-muted"
                >Unsaved changes</span>
                <UButton
                  :loading="saving"
                  :disabled="!canSave"
                  data-testid="profile-save"
                  @click="save"
                >
                  {{ exists ? 'Save profile' : 'Create profile' }}
                </UButton>
              </div>
            </div>
          </template>
        </UCard>

        <div class="lg:sticky lg:top-4 self-start">
          <ProfilePreview
            :form="form"
            :npub="npub"
          />
        </div>
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
