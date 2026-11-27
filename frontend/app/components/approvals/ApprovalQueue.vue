<script setup lang="ts">
// Requests held for approval before signing (#102). Each shows the event as the bunker will sign
// it: kind, content as plain text, tags, created_at and event id, with who asked and which app.
// Administrators, and the leads of its department (#147), approve or reject, with an optional note
// the app is shown; approving names the event id seen here, so a different event cannot be
// approved by mistake. Signers see their own requests waiting, without the buttons, and a lead
// their departments' as well. Refreshed every few seconds.
import type { PendingApproval } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'
import { bunkerRefusal } from '~/utils/bunker-error'
import { kindLabel, relativeTime } from '~/utils/connections'

const props = defineProps<{ focus?: string }>()

const auth = useAuth()
const isAdministrator = computed(() => auth.state.value.status === 'signed-in' && auth.state.value.role === 'administrator')
// Administrators decide every request, and a lead their departments' (#147); the bunker checks it.
const { leads, isLead } = useIdentities()
const mayDecide = (request: PendingApproval) => isAdministrator.value || leads(request.identity_pubkey)
const self = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value.pubkey : '')

const { data, error, refresh } = useFetch<PendingApproval[]>('/api/bunker/approvals', { key: 'approvals', default: () => [] })
const requests = computed(() => data.value ?? [])
const focused = computed(() => props.focus && requests.value.find(r => r.id === props.focus))

const notes = ref<Record<string, string>>({})
const busy = ref<string>()
const actionError = ref<string>()
const decided = ref<string>()
const now = ref(Date.now())

let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  timer = setInterval(() => {
    now.value = Date.now()
    void refresh()
  }, 5000)
  if (props.focus) {
    nextTick(() => document.querySelector(`[data-approval="${props.focus}"]`)?.scrollIntoView({ block: 'center' }))
  }
})
onBeforeUnmount(() => clearInterval(timer))

const ERRORS: Record<string, string> = {
  not_found: 'That request has already been decided, or it timed out.',
  event_changed: 'That is not the event waiting. Check it again before approving.',
  connection_ended: 'The app\'s connection has ended, so nothing was signed.'
}

async function decide(request: PendingApproval, decision: 'approve' | 'reject') {
  busy.value = request.id
  actionError.value = undefined
  decided.value = undefined
  const note = notes.value[request.id]?.trim()
  try {
    await $fetch(`/api/bunker/approvals/${encodeURIComponent(request.id)}/${decision}`, {
      method: 'POST',
      body: decision === 'approve' ? { event_id: request.event_id, ...(note ? { note } : {}) } : (note ? { note } : {})
    })
    decided.value = decision === 'approve' ? 'Approved: the bunker signed it and sent it to the app.' : 'Rejected: nothing was signed, and the app was told.'
    notes.value = Object.fromEntries(Object.entries(notes.value).filter(([id]) => id !== request.id))
  } catch (failure) {
    const code = bunkerRefusal(failure).error
    actionError.value = (code && ERRORS[code]) ?? 'The bunker did not accept that. Try again.'
  } finally {
    busy.value = undefined
    await refresh()
  }
}

function who(request: PendingApproval): string {
  if (request.member_pubkey === self.value) return 'you'
  return request.member_name ?? `${request.member_pubkey.slice(0, 8)}…`
}

function exact(iso: string): string {
  return new Date(iso).toLocaleString()
}
</script>

<template>
  <ForbiddenNotice v-if="isForbidden(error)" />
  <div
    v-else
    class="max-w-3xl space-y-4"
  >
    <UAlert
      color="neutral"
      variant="subtle"
      icon="i-lucide-shield-check"
      :title="isAdministrator || isLead ? 'Nothing below is signed until an administrator or a lead of its department approves it.' : 'An administrator must approve these before the bunker signs them.'"
      description="Requests that nobody decides time out, and nothing is signed. The app is told either way."
      data-testid="approvals-explainer"
    />
    <UAlert
      v-if="focus && !focused"
      color="neutral"
      variant="subtle"
      title="That request is no longer waiting: it was decided, or it timed out."
      data-testid="approval-gone"
    />
    <UAlert
      v-if="actionError"
      color="error"
      variant="subtle"
      :title="actionError"
      data-testid="approval-error"
    />
    <UAlert
      v-if="decided"
      color="success"
      variant="subtle"
      :title="decided"
      data-testid="approval-decided"
    />

    <p
      v-if="requests.length === 0"
      class="text-sm text-muted"
      data-testid="approvals-empty"
    >
      Nothing is waiting for approval.
    </p>

    <UCard
      v-for="request in requests"
      :key="request.id"
      :data-approval="request.id"
      :class="request.id === focus ? 'ring-2 ring-primary' : undefined"
    >
      <template #header>
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h2 class="font-semibold">
            Sign {{ kindLabel(request.kind) }}
          </h2>
          <p class="text-xs text-muted">
            <span :title="exact(request.requested_at)">asked {{ relativeTime(request.requested_at, now) }}</span>
            ·
            <span
              :title="exact(request.expires_at)"
              data-testid="approval-expires"
            >times out {{ relativeTime(request.expires_at, now) }}</span>
          </p>
        </div>
        <p class="text-sm text-muted">
          From {{ request.client_name ?? 'an unnamed app' }} (unverified), connected for {{ who(request) }}
        </p>
      </template>

      <div class="space-y-3">
        <pre
          class="whitespace-pre-wrap break-words rounded bg-elevated p-3 text-sm font-sans"
          data-testid="approval-content"
        >{{ request.content }}</pre>
        <div
          v-if="request.tags.length"
          class="text-xs"
          data-testid="approval-tags"
        >
          <p class="mb-1 text-muted">
            Tags
          </p>
          <code
            v-for="(tag, i) in request.tags"
            :key="i"
            class="mb-1 block break-all rounded bg-elevated px-1.5 py-0.5"
          >{{ JSON.stringify(tag) }}</code>
        </div>
        <p class="text-xs text-muted break-all">
          Dated {{ new Date(request.created_at * 1000).toLocaleString() }} · event id
          <code data-testid="approval-event-id">{{ request.event_id }}</code>
        </p>

        <template v-if="mayDecide(request)">
          <UFormField
            label="Note (optional)"
            help="Recorded in the audit log, and shown to the app if you reject."
          >
            <UInput
              v-model="notes[request.id]"
              class="w-full"
              maxlength="280"
              :aria-label="`Note for request ${request.id}`"
            />
          </UFormField>
          <div class="flex justify-end gap-2">
            <UButton
              color="neutral"
              variant="outline"
              icon="i-lucide-x"
              :loading="busy === request.id"
              :disabled="!!busy"
              data-testid="approval-reject"
              @click="decide(request, 'reject')"
            >
              Reject
            </UButton>
            <UButton
              icon="i-lucide-check"
              :loading="busy === request.id"
              :disabled="!!busy"
              data-testid="approval-approve"
              @click="decide(request, 'approve')"
            >
              Approve and sign
            </UButton>
          </div>
        </template>
      </div>
    </UCard>
  </div>
</template>
