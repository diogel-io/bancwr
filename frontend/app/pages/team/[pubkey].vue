<script setup lang="ts">
// A team member's Nostr profile (#77), read-only, for administrators and viewers: who the vault
// says they are (name and role, from the bunker), and their kind 0 profile (picture, name, about,
// NIP-05, links) read from their relays as the profile page reads one (#30, #62). Nothing here
// edits or publishes. Reached from each row of the team list; the route takes an npub or hex.
import { decode, npubEncode } from 'nostr-tools/nip19'
import { ROLE_LABELS } from '#shared/types/bunker'
import type { Role, TeamMember } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

const route = useRoute()

/** The key in the route as lowercase hex, or undefined when it is not a key. */
function hexOf(value: string): string | undefined {
  const trimmed = value.trim()
  if (/^[0-9a-f]{64}$/iu.test(trimmed)) return trimmed.toLowerCase()
  try {
    const decoded = decode(trimmed)
    return decoded.type === 'npub' ? decoded.data : undefined
  } catch {
    return undefined
  }
}

const pubkey = hexOf(String(route.params.pubkey ?? ''))
const npub = pubkey ? npubEncode(pubkey) : ''

// Only a vault member's profile is shown: the bunker's lookup says whether this key is one.
const { data: member, error } = await useFetch<TeamMember>(`/api/bunker/team/by-pubkey/${pubkey ?? ''}`, {
  key: `team-member-${pubkey ?? 'none'}`,
  immediate: !!pubkey
})
const notRegistered = computed(() => (error.value as { statusCode?: number } | null)?.statusCode === 404)
const roleLabel = computed(() => member.value ? ROLE_LABELS[member.value.role as Role] ?? member.value.role : '')

const profile = useMemberProfile()
const { state, form, exists, searched } = profile
onMounted(() => {
  if (pubkey && member.value) void profile.load(pubkey)
})
</script>

<template>
  <UDashboardPanel id="team-member">
    <template #header>
      <AppNavbar title="Team member" />
    </template>

    <template #body>
      <div class="max-w-3xl space-y-6">
        <UButton
          to="/team"
          variant="link"
          color="neutral"
          icon="i-lucide-arrow-left"
          class="px-0"
        >
          Back to the team
        </UButton>

        <UAlert
          v-if="!pubkey"
          color="error"
          variant="subtle"
          title="That is not a Nostr key"
          description="A member's profile address ends with their npub."
          data-testid="member-invalid-key"
        />
        <ForbiddenNotice v-else-if="isForbidden(error)" />
        <UAlert
          v-else-if="notRegistered"
          color="warning"
          variant="subtle"
          title="This key is not a member of the vault"
          :description="npub"
          data-testid="member-not-registered"
        />
        <UAlert
          v-else-if="error || !member"
          color="error"
          variant="subtle"
          title="The bunker did not answer"
          description="The member could not be looked up. Try again."
          data-testid="member-lookup-failed"
        />

        <template v-else>
          <UCard data-testid="member-summary">
            <div class="flex flex-wrap items-center gap-2">
              <h2 class="text-lg font-semibold break-words">
                {{ member.name }}
              </h2>
              <UBadge
                color="neutral"
                variant="subtle"
                data-testid="member-role"
              >
                {{ roleLabel }}
              </UBadge>
            </div>
            <p class="mt-1 font-mono text-xs text-muted break-all">
              {{ member.npub ?? member.pubkey }}
            </p>
          </UCard>

          <div
            v-if="state === 'idle' || state === 'loading'"
            class="flex items-center gap-2 text-sm text-muted"
            role="status"
            data-testid="member-profile-loading"
          >
            <UIcon
              name="i-lucide-loader-circle"
              class="size-4 animate-spin"
              aria-hidden="true"
            />
            Reading their profile from their relays…
          </div>
          <UAlert
            v-else-if="state === 'failed'"
            color="error"
            variant="subtle"
            title="Couldn't reach any of their relays"
            :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => profile.load(member!.pubkey) }]"
            data-testid="member-profile-failed"
          />
          <UAlert
            v-else-if="!exists"
            color="neutral"
            variant="subtle"
            icon="i-lucide-search-x"
            :title="`No profile found on the ${searched.reached.length + searched.failed.length} relays searched`"
            description="They may not have published one, or it is on a relay not searched."
            data-testid="member-profile-not-found"
          />
          <ProfilePreview
            v-else
            :form="form"
            :npub="npub"
            data-testid="member-profile"
          />
        </template>
      </div>
    </template>
  </UDashboardPanel>
</template>
