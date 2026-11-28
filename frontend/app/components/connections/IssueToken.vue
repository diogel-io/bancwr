<script setup lang="ts">
// Issuing a connection token (#31): for which member, a label, what the app may
// sign, which encryption methods it may call (#97), how long the token is valid, how long the
// connection lasts (#98), how fast it may use the key (#101) and which kinds need an
// administrator's approval before signing (#102). The bunker:// string it
// returns carries the token's secret and is shown once: the bunker keeps only its hash (#53), so it
// cannot be shown again. The form and the result name the identity the app signs as (#137): it is
// always one of the bunker's identities, never the member the token is for. A signer issues their
// own (#142): always for themselves, as an identity they are granted, and with no approval kinds,
// which stay an administrator's control; the bunker refuses anything else.
import { renderSVG } from 'uqr'
import { npubEncode } from 'nostr-tools/nip19'
import { isRole } from '#shared/types/bunker'
import type { BunkerIdentity, EncryptionMethod, IssueTokenRequest, IssueTokenResponse, TeamMember } from '#shared/types/bunker'
import { bunkerRefusal } from '~/utils/bunker-error'
import { ENCRYPTION_METHODS, KIND_PRESETS, LIFETIMES, RATE_LIMITS, isDecrypt, kindLabel, parseKinds } from '~/utils/connections'

const props = defineProps<{
  members: TeamMember[]
  self: string
  /**
   * Whether the signed-in member administers. A signer's form has no member select or approval
   * (#142), unless they lead the department of the identity chosen (#147).
   */
  administrator?: boolean
  /** The bunker's identities (#103). The one the token signs as is always named (#137), and with more than one it is chosen. */
  identities?: BunkerIdentity[]
  issue: (request: IssueTokenRequest) => Promise<IssueTokenResponse>
}>()

const EXPIRY = [
  { label: '1 hour', value: 1 },
  { label: '24 hours', value: 24 },
  { label: '7 days', value: 168 }
]

const forPubkey = ref(props.self)
/** The identity it signs as (#103): a pubkey, or PRIMARY. A select item cannot be empty. */
const PRIMARY = 'primary'
const identity = ref(PRIMARY)
const choosesIdentity = computed(() => (props.identities?.length ?? 0) > 1)
// Only the identities the chosen member may use, as the bunker decides it (`identity_allowed`): an
// administrator every one, anyone on a bunker with only the primary that one, anyone else those
// they are granted. The bunker refuses the rest (no_identity_access).
// A signer's list is already only those they may use, without `members` (#142): offer it whole.
const identityItems = computed(() => {
  const member = props.members.find(m => m.pubkey === forPubkey.value)
  const administrator = member?.role === 'administrator'
  return (props.identities ?? [])
    .filter(i => !props.administrator || administrator || !choosesIdentity.value || (i.members ?? []).includes(forPubkey.value))
    .map(i => ({ label: i.label, value: i.is_primary ? PRIMARY : i.pubkey }))
})
watch(identityItems, (items) => {
  if (!items.some(i => i.value === identity.value)) identity.value = items[0]?.value ?? PRIMARY
}, { immediate: true })
/** The identity chosen, once the list has loaded. */
const chosenIdentity = computed(() => (props.identities ?? []).find(i => identity.value === PRIMARY ? i.is_primary : i.pubkey === identity.value))
// Who manages a connection that signs as it (#147): administrators, and the leads of its department,
// who choose the member and may require approval. The bunker checks it again.
const manages = computed(() => props.administrator || chosenIdentity.value?.my_role === 'lead')

/** "Label (npub)" for one of the bunker's keys, or its npub alone when it is not listed. */
function describeIdentity(pubkey: string): string {
  const known = props.identities?.find(i => i.pubkey === pubkey)
  const npub = known?.npub ?? npubEncode(pubkey)
  return known ? `${known.label} (${npub})` : npub
}
// The identity the app will sign as (#137). Undefined while the identities load, or when the
// member may use none of them; the form then claims no key.
const signsAs = computed(() => {
  if (!identityItems.value.some(i => i.value === identity.value)) return undefined
  const chosen = (props.identities ?? []).find(i => identity.value === PRIMARY ? i.is_primary : i.pubkey === identity.value)
  return chosen && describeIdentity(chosen.pubkey)
})
const noIdentity = computed(() => (props.identities?.length ?? 0) > 0 && !identityItems.value.length)
const label = ref('')
const preset = ref<number | 'custom' | 'none'>(0)
const custom = ref('')
const methods = ref<EncryptionMethod[]>([])
const hours = ref(24)
/** Hours the connection lasts once the app connects (#98); 0 is until revoked. */
const lifetime = ref(0)
/** An index into RATE_LIMITS (#101); 0 is the bunker's default, which sends nothing. */
const rateLimit = ref(0)
/** Kinds held for an administrator's approval before signing (#102): some of the chosen kinds. */
const approvalKinds = ref<number[]>([])
const issuing = ref(false)
const error = ref<string>()
const issued = ref<IssueTokenResponse>()
const copied = ref(false)

// Only members with a recognised role, who can all sign (#77, #143). A row stored before #24 with
// another role grants nothing, and the bunker refuses a token for one (member_cannot_sign).
const memberItems = computed(() => props.members
  .filter(m => m.npub && isRole(m.role))
  // A lead issues for the members of the department they chose (#147); an administrator for anyone.
  .filter(m => props.administrator || (chosenIdentity.value?.members ?? []).includes(m.pubkey))
  .map(m => ({ label: m.pubkey === props.self ? `${m.name} (you)` : m.name, value: m.pubkey })))
watch(memberItems, (items) => {
  if (!items.some(i => i.value === forPubkey.value)) forPubkey.value = props.self
})
const presetItems = computed(() => [
  ...KIND_PRESETS.map((p, i) => ({ label: `${p.label} (${p.kinds.map(kindLabel).join(', ')})`, value: i as number | 'custom' | 'none' })),
  { label: 'Choose kinds…', value: 'custom' as const },
  // An app that only encrypts and decrypts (#97).
  { label: 'Nothing: encryption only', value: 'none' as const }
])
const kinds = computed<{ kinds: number[] } | { error: string }>(() => {
  if (preset.value === 'none') return { kinds: [] }
  if (preset.value === 'custom') return parseKinds(custom.value)
  return { kinds: KIND_PRESETS[preset.value]!.kinds }
})
const kindsError = computed(() => preset.value === 'custom' && custom.value.trim() && 'error' in kinds.value ? kinds.value.error : undefined)
const methodsError = computed(() => preset.value === 'none' && !methods.value.length ? 'Choose at least one method, or kinds the app may sign.' : undefined)
const decrypts = computed(() => methods.value.some(isDecrypt))
const chosenKinds = computed(() => 'kinds' in kinds.value ? kinds.value.kinds : [])
// Only kinds the app may sign can need approval: drop any no longer chosen.
watch(chosenKinds, (now) => {
  approvalKinds.value = approvalKinds.value.filter(k => now.includes(k))
})

function toggleApproval(kind: number, on: boolean | 'indeterminate') {
  const rest = approvalKinds.value.filter(k => k !== kind)
  approvalKinds.value = on === true ? [...rest, kind].sort((a, b) => a - b) : rest
}
const canIssue = computed(() => !!label.value.trim() && 'kinds' in kinds.value && (kinds.value.kinds.length + methods.value.length) > 0 && !issuing.value)

function toggleMethod(method: EncryptionMethod, on: boolean | 'indeterminate') {
  const rest = methods.value.filter(m => m !== method)
  // Kept in the bunker's order, so what is sent matches what it lists back.
  methods.value = on === true ? ENCRYPTION_METHODS.map(m => m.value).filter(m => m === method || rest.includes(m)) : rest
}
// An image, not injected markup: the SVG is uqr's own output, but nothing here needs v-html.
const qr = computed(() => issued.value ? `data:image/svg+xml;utf8,${encodeURIComponent(renderSVG(issued.value.uri, { border: 1 }))}` : '')
/** The identity the issued token signs as: the response's, or the bunker:// host, the same hex key. */
const issuedSignsAs = computed(() => {
  if (!issued.value) return undefined
  let pubkey = issued.value.identity_pubkey
  if (!pubkey) {
    try {
      pubkey = new URL(issued.value.uri).hostname
    } catch {
      return undefined
    }
  }
  return /^[0-9a-f]{64}$/.test(pubkey) ? describeIdentity(pubkey) : undefined
})

const ERRORS: Record<string, string> = {
  nip46_disabled: 'NIP-46 is turned off on this bunker (NIP46_ENABLED=true turns it on), or it has no relays: an administrator adds them under Config, Bunker relays.',
  not_registered: 'Connections can only be issued for members of the vault.',
  member_cannot_sign: 'This member has no recognised role and cannot sign.',
  invalid_lifetime: 'A connection can last 1 hour to 365 days, or until revoked.',
  invalid_rate_limit: 'A connection can make 1 to 600 requests a minute, with a burst of 1 to 100.',
  invalid_approval_kinds: 'Only kinds the app may sign can need approval.',
  no_identity_access: 'That member may not sign as this identity. Grant it on their team page first.',
  forbidden_member: 'You can only issue connections for yourself.',
  approval_admin_only: 'Only an administrator or a lead of this department can require approval.',
  not_your_department: 'That member is not in this department.',
  invalid_identity: 'That is not one of this bunker\'s identities.',
  invalid_kinds: 'Choose at least one kind the app may sign, or a method it may call.'
}

/** The chosen rate limit, or nothing for the default. */
function rateLimitRequest(): { rate_per_minute?: number, rate_burst?: number } {
  const chosen = RATE_LIMITS.find(r => r.value === rateLimit.value)
  return chosen && chosen.value ? { rate_per_minute: chosen.perMinute, rate_burst: chosen.burst } : {}
}

async function submit() {
  if (!canIssue.value || !('kinds' in kinds.value)) return
  issuing.value = true
  error.value = undefined
  try {
    issued.value = await props.issue({
      for_pubkey: manages.value ? forPubkey.value : props.self,
      label: label.value.trim(),
      kinds: kinds.value.kinds,
      methods: methods.value,
      expires_in_hours: hours.value,
      ...(lifetime.value ? { connection_lifetime_hours: lifetime.value } : {}),
      ...rateLimitRequest(),
      ...(manages.value && approvalKinds.value.length ? { approval_kinds: approvalKinds.value } : {}),
      ...(identity.value !== PRIMARY ? { identity: identity.value } : {})
    })
    copied.value = false
  } catch (failure) {
    const refusal = bunkerRefusal(failure)
    error.value = (refusal.error && ERRORS[refusal.error]) ?? refusal.message ?? 'The token could not be issued.'
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
  methods.value = []
  lifetime.value = 0
  rateLimit.value = 0
  approvalKinds.value = []
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
        <p
          v-if="issuedSignsAs"
          class="text-sm break-all"
          data-testid="issued-identity"
        >
          Signs as {{ issuedSignsAs }}
        </p>
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
        v-if="manages"
        label="For"
        help="The member this token is issued to. Their role, identity grants and approvals apply, and the log names them. The app does not sign as them."
      >
        <USelect
          id="issue-token-for"
          v-model="forPubkey"
          :items="memberItems"
          class="w-full"
        />
      </UFormField>
      <UFormField
        :label="choosesIdentity ? 'Sign as' : 'Identity'"
        :help="choosesIdentity ? (administrator ? 'The identity the app signs as. Only those this member may use are offered.' : 'The identity the app signs as. Only those you may use are offered.') : undefined"
        data-testid="issue-token-signs-as-field"
      >
        <div class="space-y-1.5">
          <div
            v-if="choosesIdentity"
            data-testid="issue-token-identity"
          >
            <USelect
              id="issue-token-identity"
              v-model="identity"
              :items="identityItems"
              class="w-full"
            />
          </div>
          <p
            v-if="signsAs"
            class="text-sm break-all"
            data-testid="issue-token-signs-as"
          >
            Signs as {{ signsAs }}
          </p>
          <p
            v-else-if="noIdentity"
            class="text-sm text-error"
            data-testid="issue-token-signs-as"
          >
            {{ administrator ? 'This member may not use any of this bunker\'s identities yet. Grant one on their team page first.' : 'You may not use any of this bunker\'s identities yet. Ask an administrator to grant you one.' }}
          </p>
          <p
            v-else
            class="text-sm"
            data-testid="issue-token-signs-as"
          >
            Signs as the bunker's primary identity.
          </p>
        </div>
      </UFormField>
      <UFormField
        label="Label"
        help="To recognise it later, e.g. Damus on Gary's phone."
      >
        <UInput
          id="issue-token-label"
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
            id="issue-token-kinds"
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
      <UFormField
        v-if="manages && chosenKinds.length"
        label="Needs approval"
        help="An administrator or a lead of this department approves each of these on the Approvals page before the bunker signs it."
        data-testid="issue-token-approval"
      >
        <div class="space-y-1.5">
          <UCheckbox
            v-for="kind in chosenKinds"
            :id="`issue-token-approval-${kind}`"
            :key="kind"
            :model-value="approvalKinds.includes(kind)"
            :label="kindLabel(kind)"
            :data-kind="kind"
            @update:model-value="on => toggleApproval(kind, on)"
          />
        </div>
      </UFormField>
      <UFormField
        label="May also"
        help="Each is a separate permission. Encrypting lets the app send private messages as the brand."
        :error="methodsError"
        data-testid="issue-token-methods"
      >
        <div class="space-y-1.5">
          <UCheckbox
            v-for="method in ENCRYPTION_METHODS"
            :id="`issue-token-${method.value}`"
            :key="method.value"
            :model-value="methods.includes(method.value)"
            :label="method.label"
            :data-method="method.value"
            @update:model-value="on => toggleMethod(method.value, on)"
          />
        </div>
      </UFormField>
      <UFormField
        label="Valid for"
        help="How long the token can be used to connect."
      >
        <USelect
          id="issue-token-expiry"
          v-model="hours"
          :items="EXPIRY"
          class="w-full"
        />
      </UFormField>
      <UFormField
        label="Connection lasts"
        help="Counted from when the app connects. It cannot be extended: issue a new token instead."
      >
        <USelect
          id="issue-token-lifetime"
          v-model="lifetime"
          :items="LIFETIMES"
          class="w-full"
        />
      </UFormField>
      <UFormField
        label="Rate limit"
        help="How many signing and encryption requests the app may make. Past it, the app is told to wait."
      >
        <USelect
          id="issue-token-rate-limit"
          v-model="rateLimit"
          :items="RATE_LIMITS"
          class="w-full"
        />
      </UFormField>
    </div>
    <UAlert
      v-if="decrypts"
      color="warning"
      variant="subtle"
      icon="i-lucide-mail-open"
      title="This app can read the brand's private messages"
      description="Reading encrypted messages works for every message sent to the brand's key, from anyone. Each read is recorded in the log, without its content."
      data-testid="issue-token-decrypt-warning"
    />
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
