<script setup lang="ts">
// Issuing a connection token (#31, administrators): for which member, a label, what the app may
// sign, and for how long. The bunker:// string it returns carries the token's secret and is shown
// once: the bunker keeps only its hash (#53), so it cannot be shown again.
import { renderSVG } from 'uqr'
import type { IssueTokenRequest, IssueTokenResponse, TeamMember } from '#shared/types/bunker'
import { KIND_PRESETS, kindLabel, parseKinds } from '~/utils/connections'

const props = defineProps<{
  members: TeamMember[]
  self: string
  issue: (request: IssueTokenRequest) => Promise<IssueTokenResponse>
}>()

const EXPIRY = [
  { label: '1 hour', value: 1 },
  { label: '24 hours', value: 24 },
  { label: '7 days', value: 168 }
]

const forPubkey = ref(props.self)
const label = ref('')
const preset = ref<number | 'custom'>(0)
const custom = ref('')
const hours = ref(24)
const issuing = ref(false)
const error = ref<string>()
const issued = ref<IssueTokenResponse>()
const copied = ref(false)

const memberItems = computed(() => props.members
  .filter(m => m.npub)
  .map(m => ({ label: m.pubkey === props.self ? `${m.name} (you)` : m.name, value: m.pubkey })))
const presetItems = computed(() => [
  ...KIND_PRESETS.map((p, i) => ({ label: `${p.label} (${p.kinds.map(kindLabel).join(', ')})`, value: i as number | 'custom' })),
  { label: 'Choose kinds…', value: 'custom' as const }
])
const kinds = computed(() => preset.value === 'custom' ? parseKinds(custom.value) : { kinds: KIND_PRESETS[preset.value]!.kinds })
const kindsError = computed(() => preset.value === 'custom' && custom.value.trim() && 'error' in kinds.value ? kinds.value.error : undefined)
const canIssue = computed(() => !!label.value.trim() && 'kinds' in kinds.value && !issuing.value)
// An image, not injected markup: the SVG is uqr's own output, but nothing here needs v-html.
const qr = computed(() => issued.value ? `data:image/svg+xml;utf8,${encodeURIComponent(renderSVG(issued.value.uri, { border: 1 }))}` : '')

const ERRORS: Record<string, string> = {
  nip46_disabled: 'NIP-46 is turned off on this bunker (NIP46_ENABLED=true turns it on), or it has no relays: an administrator adds them under Config, Bunker relays.',
  not_registered: 'Connections can only be issued for members of the vault.'
}

async function submit() {
  if (!canIssue.value || !('kinds' in kinds.value)) return
  issuing.value = true
  error.value = undefined
  try {
    issued.value = await props.issue({ for_pubkey: forPubkey.value, label: label.value.trim(), kinds: kinds.value.kinds, expires_in_hours: hours.value })
    copied.value = false
  } catch (failure) {
    const data = (failure as { data?: { error?: string, message?: string } }).data
    error.value = (data?.error && ERRORS[data.error]) ?? data?.message ?? 'The token could not be issued.'
  } finally {
    issuing.value = false
  }
}

async function copy() {
  if (!issued.value) return
  await navigator.clipboard.writeText(issued.value.uri)
  copied.value = true
}

/** Closing the result drops the string for good. */
function done() {
  issued.value = undefined
  label.value = ''
}
</script>

<template>
  <div
    v-if="issued"
    class="space-y-3"
    data-testid="issued-token"
  >
    <UAlert
      color="warning"
      variant="subtle"
      icon="i-lucide-key-round"
      title="Shown once"
      description="Give this to the app now. Anyone with it can connect until it is used or expires, and it cannot be shown again."
    />
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start">
      <img
        :src="qr"
        alt="QR code of the connection string"
        class="size-44 shrink-0 rounded-md bg-white p-1"
        data-testid="issued-qr"
      >
      <div class="min-w-0 flex-1 space-y-2">
        <code
          class="block rounded bg-elevated p-2 text-xs break-all"
          data-testid="issued-uri"
        >{{ issued.uri }}</code>
        <div class="flex gap-2">
          <UButton
            size="sm"
            :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
            @click="copy"
          >
            {{ copied ? 'Copied' : 'Copy' }}
          </UButton>
          <UButton
            size="sm"
            color="neutral"
            variant="outline"
            @click="done"
          >
            Done
          </UButton>
        </div>
      </div>
    </div>
  </div>

  <form
    v-else
    class="space-y-4"
    data-testid="issue-token"
    @submit.prevent="submit"
  >
    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      data-testid="issue-error"
    />
    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <UFormField
        label="For"
        help="The member this app is connected for. It signs as the bunker's key."
      >
        <USelect
          v-model="forPubkey"
          :items="memberItems"
          class="w-full"
        />
      </UFormField>
      <UFormField
        label="Label"
        help="To recognise it later, e.g. Damus on Gary's phone."
      >
        <UInput
          v-model="label"
          class="w-full"
          maxlength="100"
        />
      </UFormField>
      <UFormField
        label="May sign"
        :error="kindsError"
      >
        <div class="space-y-2">
          <USelect
            v-model="preset"
            :items="presetItems"
            class="w-full"
          />
          <UInput
            v-if="preset === 'custom'"
            v-model="custom"
            class="w-full"
            placeholder="1, 7, 30023"
            aria-label="Kinds"
          />
        </div>
      </UFormField>
      <UFormField label="Valid for">
        <USelect
          v-model="hours"
          :items="EXPIRY"
          class="w-full"
        />
      </UFormField>
    </div>
    <div class="flex justify-end">
      <UButton
        type="submit"
        icon="i-lucide-key-round"
        :loading="issuing"
        :disabled="!canIssue"
      >
        Issue token
      </UButton>
    </div>
  </form>
</template>
