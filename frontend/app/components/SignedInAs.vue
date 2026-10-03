<script setup lang="ts">
// Whose data a page edits (#70): the signed-in key, with its name when its profile is found
// quickly. Shown on /profile, /follows and /relays, so the identity is on the page, not only in
// the sidebar footer. The lookup is the footer's, shared (#72), so it isn't repeated per page.
const auth = useAuth()
const { profile, load } = useSignedInProfile()
const user = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value : undefined)

onMounted(() => void load())

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
