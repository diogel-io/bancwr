<script setup lang="ts">
// Whose data a page edits (#70): the signed-in key, with its name when its profile is found
// quickly. Shown on /profile, /follows and /relays, so the identity is on the page, not only in
// the sidebar footer.
import type { FollowProfile } from '~/utils/follows'
import { lookupKeyProfile, lookupRelays, queryRelays } from '~/utils/key-profile'

const auth = useAuth()
const config = useRuntimeConfig().public
const user = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value : undefined)
const profile = ref<FollowProfile>()

onMounted(async () => {
  if (!user.value) return
  profile.value = await lookupKeyProfile(user.value.pubkey, lookupRelays(config.profileRelays, config.indexerRelays), queryRelays)
})

const short = computed(() => user.value ? `${user.value.npub.slice(0, 12)}…${user.value.npub.slice(-6)}` : '')
</script>

<template>
  <p
    v-if="user"
    class="flex flex-wrap items-center gap-x-2 text-sm text-muted"
    data-testid="signed-in-as"
  >
    <span>Signed in as <strong class="text-default">{{ profile?.name ?? short }}</strong></span>
    <span
      class="font-mono text-xs break-all"
      :title="user.npub"
    >{{ user.npub }}</span>
  </p>
</template>
