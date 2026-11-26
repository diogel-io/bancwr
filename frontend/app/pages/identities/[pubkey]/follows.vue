<script setup lang="ts">
// An identity's follows, for administrators (#150): the member's own editor (pages/follows.vue), which
// reads the route's pubkey (useEditTarget) and edits the identity, signed by the bunker as it. The
// key is read once, so each identity gets its own page instance.
definePageMeta({ key: route => route.path })
</script>

<template>
  <FollowsEditor />
</template>
