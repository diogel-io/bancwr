<script setup lang="ts">
// Apps connected to the bunker for the signed-in member (#31), over #53's connections: every app
// signs as one of the bunker's identities, authorised for a member. For administrators and signers
// (#77): signers see and revoke theirs, administrators all. Administrators issue tokens for admins
// and signers; signers issue, see and revoke their own (#142).
import { isForbidden } from '~/utils/access'

const {
  pubkey, isAdministrator, connections, tokens, team, memberName, revoke, revokeToken, issue
} = useConnections()

// The identities (#103): named on each row, and chosen when issuing, only once there are several.
const { list: identities, several, label: identityLabel } = useIdentities()

const showEnded = ref(false)
const actionError = ref<string>()

const list = computed(() => connections.data.value ?? [])
const shown = computed(() => list.value.filter(c => showEnded.value || !c.revoked_at))
const ended = computed(() => list.value.filter(c => c.revoked_at).length)
const unused = computed(() => (tokens.data.value ?? []).filter(t => !t.used_at && !t.revoked_at && new Date(t.expires_at).getTime() > Date.now()))

async function act(work: () => Promise<void>) {
  actionError.value = undefined
  try {
    await work()
  } catch (failure) {
    const status = (failure as { statusCode?: number }).statusCode
    actionError.value = status === 404
      ? 'That connection has already ended, or is not yours to revoke.'
      : 'The bunker did not accept that. Try again.'
  }
}
</script>

<template>
  <UDashboardPanel id="connections">
    <template #header>
      <AppNavbar title="Connected apps" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(connections.error.value)" />
      <div
        v-else
        class="max-w-3xl space-y-6"
      >
        <UAlert
          color="neutral"
          variant="subtle"
          icon="i-lucide-info"
          title="These apps sign as this bunker's key, not your own."
          description="Each was authorised for you, by you or an administrator, and can sign only the kinds listed."
          data-testid="connections-explainer"
        />

        <UAlert
          v-if="actionError"
          color="error"
          variant="subtle"
          :title="actionError"
          data-testid="connections-error"
        />

        <UCard>
          <template #header>
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h2 class="font-semibold">
                {{ isAdministrator ? 'Connected apps' : 'Apps connected for you' }}
              </h2>
              <USwitch
                v-if="ended > 0"
                v-model="showEnded"
                :label="`Show ended (${ended})`"
              />
            </div>
          </template>

          <ul
            v-if="shown.length > 0"
            class="divide-y divide-default"
            data-testid="connections-list"
          >
            <ConnectionsConnectionRow
              v-for="connection in shown"
              :key="connection.id"
              :connection="connection"
              :for-name="isAdministrator ? memberName(connection.for_pubkey) : undefined"
              :revoked-by-name="connection.revoked_by ? memberName(connection.revoked_by) : undefined"
              :identity-label="several ? identityLabel(connection.identity_pubkey) : undefined"
              @revoke="act(() => revoke(connection.id))"
            />
          </ul>
          <div
            v-else
            class="text-sm text-muted"
            data-testid="connections-empty"
          >
            <p>No apps are connected {{ isAdministrator ? 'to this bunker' : 'for you' }}.</p>
            <p>
              Issue a connection token below to connect one.
            </p>
          </div>
        </UCard>

        <UCard>
          <template #header>
            <h2 class="font-semibold">
              Issue a connection token
            </h2>
            <p class="text-sm text-muted">
              A single-use bunker:// string for one app, which may then sign only the kinds chosen here.
            </p>
          </template>
          <ConnectionsIssueToken
            :members="team.data.value ?? []"
            :self="pubkey"
            :administrator="isAdministrator"
            :issue="issue"
            :identities="identities"
          />
        </UCard>

        <UCard v-if="unused.length > 0">
          <template #header>
            <h2 class="font-semibold">
              Unused tokens
            </h2>
          </template>
          <ul
            class="divide-y divide-default"
            data-testid="tokens-list"
          >
            <ConnectionsTokenRow
              v-for="token in unused"
              :key="token.id"
              :token="token"
              :for-name="memberName(token.for_pubkey)"
              :identity-label="several ? identityLabel(token.identity_pubkey) : undefined"
              @revoke="act(() => revokeToken(token.id))"
            />
          </ul>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
