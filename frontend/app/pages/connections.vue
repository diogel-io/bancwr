<script setup lang="ts">
// Apps connected to the bunker for the signed-in member (#31), over #53's connections: every app
// signs as the bunker's key, authorised by an administrator for a member. Users see theirs and
// administrators all; anyone revokes their own, administrators any. Administrators issue tokens.
import { isForbidden } from '~/utils/access'

const {
  pubkey, isAdministrator, connections, tokens, team, memberName, revoke, revokeToken, issue
} = useConnections()

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
          description="Each was authorised by an administrator for you, and can sign only the kinds listed."
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
              @revoke="act(() => revoke(connection.id))"
            />
          </ul>
          <div
            v-else
            class="text-sm text-muted"
            data-testid="connections-empty"
          >
            <p>No apps are connected {{ isAdministrator ? 'to this bunker' : 'for you' }}.</p>
            <p v-if="isAdministrator">
              Issue a connection token below to connect one.
            </p>
            <p v-else>
              Ask an administrator for a connection token to connect one.
            </p>
          </div>
        </UCard>

        <template v-if="isAdministrator">
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
              :issue="issue"
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
                @revoke="act(() => revokeToken(token.id))"
              />
            </ul>
          </UCard>
        </template>
      </div>
    </template>
  </UDashboardPanel>
</template>
