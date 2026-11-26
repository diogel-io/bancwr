<script setup lang="ts">
// Connecting an app from its own nostrconnect:// code (#148), for apps that offer no way to paste a
// bunker:// string. The member pastes the code, and the bunker reads it: the app's name and address
// (both unverified, as for any connection), its key, the relays it chose and what it asked for. The
// app's logo is never loaded: its address is the app's choice, and fetching it would tell whoever
// runs it this console's IP address. The member then chooses the identity and what the connection
// may do, under exactly a token's rules (#142): a signer for themselves, as identities they may use,
// without approval kinds. What the app asked for only pre-fills the form, and the bunker grants only
// what the member confirms of it. On confirming, the bunker answers the app on its relays.
import { npubEncode } from 'nostr-tools/nip19'
import { isRole } from '#shared/types/bunker'
import type { BunkerIdentity, EncryptionMethod, Nip46Connection, NostrConnectPreview, NostrConnectRequest, TeamMember } from '#shared/types/bunker'
import { bunkerRefusal } from '~/utils/bunker-error'
import { ENCRYPTION_METHODS, LIFETIMES, RATE_LIMITS, isDecrypt, kindLabel, methodLabel, parseKinds } from '~/utils/connections'

const props = defineProps<{
  members: TeamMember[]
  self: string
  /** Whether the signed-in member administers. A signer's form has no member select or approval (#142). */
  administrator?: boolean
  /** The bunker's identities (#103); with more than one, the member chooses. */
  identities?: BunkerIdentity[]
  preview: (uri: string) => Promise<NostrConnectPreview>
  connect: (request: NostrConnectRequest) => Promise<Nip46Connection>
}>()

const code = ref('')
const reading = ref(false)
const shown = ref<NostrConnectPreview>()
const error = ref<string>()
const connected = ref<{ name: string, signsAs?: string }>()

const forPubkey = ref(props.self)
/** The identity it signs as: a pubkey, or PRIMARY. A select item cannot be empty. */
const PRIMARY = 'primary'
const identity = ref(PRIMARY)
const choosesIdentity = computed(() => (props.identities?.length ?? 0) > 1)
// Only the identities the chosen member may use, as IssueToken offers them: the bunker refuses the
// rest (no_identity_access).
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
const signsAs = computed(() => {
  if (!identityItems.value.some(i => i.value === identity.value)) return undefined
  const chosen = (props.identities ?? []).find(i => identity.value === PRIMARY ? i.is_primary : i.pubkey === identity.value)
  return chosen && `${chosen.label} (${chosen.npub ?? npubEncode(chosen.pubkey)})`
})
const noIdentity = computed(() => (props.identities?.length ?? 0) > 0 && !identityItems.value.length)
const memberItems = computed(() => props.members
  .filter(m => m.npub && isRole(m.role))
  .map(m => ({ label: m.pubkey === props.self ? `${m.name} (you)` : m.name, value: m.pubkey })))

const kindsText = ref('')
const methods = ref<EncryptionMethod[]>([])
const lifetime = ref(0)
const rateLimit = ref(0)
const approvalKinds = ref<number[]>([])
const connecting = ref(false)

const kinds = computed(() => parseKinds(kindsText.value))
const chosenKinds = computed(() => 'kinds' in kinds.value ? kinds.value.kinds : [])
const kindsError = computed(() => kindsText.value.trim() && 'error' in kinds.value ? kinds.value.error : undefined)
watch(chosenKinds, (now) => {
  approvalKinds.value = approvalKinds.value.filter(k => now.includes(k))
})
const decrypts = computed(() => methods.value.some(isDecrypt))
/** Whether the app asked for anything the bunker grants: then only that can be granted. */
const asked = computed(() => !!shown.value && (shown.value.requested_any_kind || shown.value.requested_kinds.length > 0 || shown.value.requested_methods.length > 0))
// Choices the app did not ask for, which the bunker would drop.
const unasked = computed(() => {
  const app = shown.value
  if (!app || !asked.value) return []
  return [
    ...(app.requested_any_kind ? [] : chosenKinds.value.filter(k => !app.requested_kinds.includes(k)).map(kindLabel)),
    ...methods.value.filter(m => !app.requested_methods.includes(m)).map(methodLabel)
  ]
})
const canConnect = computed(() => !!shown.value && !shown.value.already_connected && 'kinds' in kinds.value
  && (chosenKinds.value.length + methods.value.length) > 0 && !connecting.value && !noIdentity.value)
const appName = computed(() => shown.value?.name ?? 'This app')
const appNpub = computed(() => shown.value ? npubEncode(shown.value.client_pubkey) : '')
const otherRelays = computed(() => (shown.value?.relays ?? []).filter(r => !r.bunker))

function toggleMethod(method: EncryptionMethod, on: boolean | 'indeterminate') {
  const rest = methods.value.filter(m => m !== method)
  methods.value = on === true ? ENCRYPTION_METHODS.map(m => m.value).filter(m => m === method || rest.includes(m)) : rest
}

function toggleApproval(kind: number, on: boolean | 'indeterminate') {
  const rest = approvalKinds.value.filter(k => k !== kind)
  approvalKinds.value = on === true ? [...rest, kind].sort((a, b) => a - b) : rest
}

const ERRORS: Record<string, string> = {
  nip46_disabled: 'NIP-46 is turned off on this bunker (NIP46_ENABLED=true turns it on), so it cannot answer an app.',
  already_connected: 'That app is already connected. Revoke its connection first, or ask the app for a new code.',
  nothing_grantable: 'None of the kinds or methods you chose are ones the app asked for.',
  connect_undelivered: 'None of the app\'s relays took the bunker\'s answer, so nothing was connected. Check the app is still waiting, or ask it for a new code.',
  not_registered: 'Connections can only be made for members of the vault.',
  member_cannot_sign: 'This member has no recognised role and cannot sign.',
  no_identity_access: 'That member may not sign as this identity. Grant it on their team page first.',
  forbidden_member: 'You can only connect apps for yourself.',
  approval_admin_only: 'Only an administrator can require approval.',
  invalid_identity: 'That is not one of this bunker\'s identities.',
  identity_unavailable: 'That identity is not available in this bunker yet: it loads when the bunker restarts.',
  invalid_kinds: 'Choose at least one kind the app may sign, or a method it may call.',
  invalid_lifetime: 'A connection can last 1 hour to 365 days, or until revoked.',
  invalid_rate_limit: 'A connection can make 1 to 600 requests a minute, with a burst of 1 to 100.'
}

function refusal(failure: unknown, fallback: string): string {
  const { error: code, message } = bunkerRefusal(failure)
  // A code's own problem is worded by the bunker, for the code that was pasted.
  if (code === 'invalid_nostrconnect_uri' && message) return message
  return (code && ERRORS[code]) ?? message ?? fallback
}

async function read() {
  if (!code.value.trim() || reading.value) return
  reading.value = true
  error.value = undefined
  connected.value = undefined
  try {
    const app = await props.preview(code.value.trim())
    shown.value = app
    // A pre-fill only: the member decides.
    kindsText.value = app.requested_kinds.join(', ')
    methods.value = [...app.requested_methods]
    approvalKinds.value = []
  } catch (failure) {
    shown.value = undefined
    error.value = refusal(failure, 'The code could not be read.')
  } finally {
    reading.value = false
  }
}

function rateLimitRequest(): { rate_per_minute?: number, rate_burst?: number } {
  const chosen = RATE_LIMITS.find(r => r.value === rateLimit.value)
  return chosen && chosen.value ? { rate_per_minute: chosen.perMinute, rate_burst: chosen.burst } : {}
}

async function submit() {
  if (!canConnect.value || !shown.value) return
  connecting.value = true
  error.value = undefined
  try {
    await props.connect({
      uri: code.value.trim(),
      for_pubkey: props.administrator ? forPubkey.value : props.self,
      kinds: chosenKinds.value,
      methods: methods.value,
      ...(lifetime.value ? { connection_lifetime_hours: lifetime.value } : {}),
      ...rateLimitRequest(),
      ...(props.administrator && approvalKinds.value.length ? { approval_kinds: approvalKinds.value } : {}),
      ...(identity.value !== PRIMARY ? { identity: identity.value } : {})
    })
    connected.value = { name: appName.value, signsAs: signsAs.value }
    reset()
  } catch (failure) {
    error.value = refusal(failure, 'The app could not be connected.')
  } finally {
    connecting.value = false
  }
}

function reset() {
  code.value = ''
  shown.value = undefined
  kindsText.value = ''
  methods.value = []
  lifetime.value = 0
  rateLimit.value = 0
  approvalKinds.value = []
}
</script>

<template>
  <div
    class="space-y-4"
    data-testid="connect-app"
  >
    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      :title="error"
      data-testid="connect-app-error"
    />
    <UAlert
      v-if="connected"
      color="success"
      variant="subtle"
      icon="i-lucide-plug"
      :title="`Connected ${connected.name}${connected.signsAs ? ` as ${connected.signsAs}` : ''}.`"
      description="The app should show it is connected. It is listed above, where you can revoke it."
      data-testid="connect-app-done"
    />

    <form
      class="space-y-2"
      data-testid="connect-app-read"
      @submit.prevent="read"
    >
      <UFormField
        label="The app's nostrconnect:// code"
        help="Copy it from the app, where it is shown beside a QR code. It starts with nostrconnect://."
      >
        <UTextarea
          id="connect-app-code"
          v-model="code"
          :rows="2"
          autoresize
          class="w-full font-mono"
          placeholder="nostrconnect://…"
          @update:model-value="shown = undefined"
        />
      </UFormField>
      <div class="flex justify-end">
        <UButton
          type="submit"
          color="neutral"
          variant="outline"
          icon="i-lucide-scan-search"
          :loading="reading"
          :disabled="!code.trim()"
        >
          Read code
        </UButton>
      </div>
    </form>

    <form
      v-if="shown"
      class="space-y-4 border-t border-default pt-4"
      data-testid="connect-app-confirm"
      @submit.prevent="submit"
    >
      <div
        class="space-y-1"
        data-testid="connect-app-preview"
      >
        <div class="flex flex-wrap items-center gap-2">
          <p class="text-sm font-medium break-all">
            {{ appName }}
          </p>
          <UBadge
            v-if="shown.name || shown.url"
            size="sm"
            color="warning"
            variant="subtle"
            data-testid="connect-app-unverified"
          >
            Unverified
          </UBadge>
        </div>
        <p
          v-if="shown.url"
          class="text-xs text-muted break-all"
        >
          Says it is {{ shown.url }}
        </p>
        <p class="text-xs text-muted font-mono break-all">
          App key {{ appNpub }}
        </p>
        <p
          class="text-xs text-muted"
          data-testid="connect-app-asked"
        >
          <template v-if="!asked">
            It asks for nothing in particular: it gets only what you choose below.
          </template>
          <template v-else>
            It asks to {{ [
              ...(shown.requested_any_kind ? ['sign any kind'] : shown.requested_kinds.length ? [`sign ${shown.requested_kinds.map(kindLabel).join(', ')}`] : []),
              ...shown.requested_methods.map(methodLabel)
            ].join('; ') }}. You choose what it gets, and only what it asked for can be granted.
          </template>
        </p>
      </div>
      <UAlert
        v-if="shown.already_connected"
        color="warning"
        variant="subtle"
        title="That app is already connected"
        description="Revoke its connection first, or ask the app for a new code."
        data-testid="connect-app-already"
      />
      <UAlert
        v-if="otherRelays.length"
        color="neutral"
        variant="subtle"
        icon="i-lucide-radio-tower"
        :title="`The bunker will also use ${otherRelays.length === 1 ? 'a relay' : 'relays'} the app chose`"
        :description="`${otherRelays.map(r => r.url).join(', ')}. The bunker listens there for this app only, while it is connected, and never signs in to ${otherRelays.length === 1 ? 'it' : 'them'}. The app is asked to move to the bunker's own relays.`"
        data-testid="connect-app-relays"
      />

      <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
        <UFormField
          v-if="administrator"
          label="For"
          help="The member the connection is made for. Their role, identity grants and approvals apply, and the log names them."
        >
          <USelect
            id="connect-app-for"
            v-model="forPubkey"
            :items="memberItems"
            class="w-full"
          />
        </UFormField>
        <UFormField
          :label="choosesIdentity ? 'Sign as' : 'Identity'"
          :help="choosesIdentity ? 'The identity that answers the app and signs for it.' : undefined"
        >
          <div class="space-y-1.5">
            <USelect
              v-if="choosesIdentity"
              id="connect-app-identity"
              v-model="identity"
              :items="identityItems"
              class="w-full"
            />
            <p
              v-if="signsAs"
              class="text-sm break-all"
              data-testid="connect-app-signs-as"
            >
              Signs as {{ signsAs }}
            </p>
            <p
              v-else-if="noIdentity"
              class="text-sm text-error"
              data-testid="connect-app-signs-as"
            >
              {{ administrator ? 'This member may not use any of this bunker\'s identities yet. Grant one on their team page first.' : 'You may not use any of this bunker\'s identities yet. Ask an administrator to grant you one.' }}
            </p>
            <p
              v-else
              class="text-sm"
              data-testid="connect-app-signs-as"
            >
              Signs as the bunker's primary identity.
            </p>
          </div>
        </UFormField>
        <UFormField
          label="May sign"
          help="Kinds, separated by commas. Filled in with what the app asked for."
          :error="kindsError"
        >
          <UInput
            id="connect-app-kinds"
            v-model="kindsText"
            class="w-full"
            placeholder="1, 7"
          />
        </UFormField>
        <UFormField
          v-if="administrator && chosenKinds.length"
          label="Needs approval"
          help="An administrator approves each of these on the Approvals page before the bunker signs it."
          data-testid="connect-app-approval"
        >
          <div class="space-y-1.5">
            <UCheckbox
              v-for="kind in chosenKinds"
              :id="`connect-app-approval-${kind}`"
              :key="kind"
              :model-value="approvalKinds.includes(kind)"
              :label="kindLabel(kind)"
              @update:model-value="on => toggleApproval(kind, on)"
            />
          </div>
        </UFormField>
        <UFormField
          label="May also"
          help="Each is a separate permission. Encrypting lets the app send private messages as the brand."
          data-testid="connect-app-methods"
        >
          <div class="space-y-1.5">
            <UCheckbox
              v-for="method in ENCRYPTION_METHODS"
              :id="`connect-app-${method.value}`"
              :key="method.value"
              :model-value="methods.includes(method.value)"
              :label="method.label"
              :data-method="method.value"
              @update:model-value="on => toggleMethod(method.value, on)"
            />
          </div>
        </UFormField>
        <UFormField
          label="Connection lasts"
          help="Counted from now. It cannot be extended: connect the app again instead."
        >
          <USelect
            id="connect-app-lifetime"
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
            id="connect-app-rate-limit"
            v-model="rateLimit"
            :items="RATE_LIMITS"
            class="w-full"
          />
        </UFormField>
      </div>
      <UAlert
        v-if="unasked.length"
        color="neutral"
        variant="subtle"
        :title="`The app did not ask for ${unasked.join(', ')}, so ${unasked.length === 1 ? 'it is' : 'they are'} not granted.`"
        data-testid="connect-app-unasked"
      />
      <UAlert
        v-if="decrypts"
        color="warning"
        variant="subtle"
        icon="i-lucide-mail-open"
        title="This app can read the brand's private messages"
        description="Reading encrypted messages works for every message sent to the brand's key, from anyone. Each read is recorded in the log, without its content."
      />
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="reset"
        >
          Cancel
        </UButton>
        <UButton
          type="submit"
          icon="i-lucide-plug"
          :loading="connecting"
          :disabled="!canConnect"
        >
          Connect {{ appName }}
        </UButton>
      </div>
    </form>
  </div>
</template>
