<script setup lang="ts">
// Shown where the bunker refused a request the page expected to make (#26): the middleware only
// lets a role onto pages it may open, so this means the role changed since the page was opened.
// The next navigation re-reads the role and moves the user on.
</script>

<template>
  <UAlert
    data-testid="forbidden-notice"
    color="warning"
    variant="subtle"
    icon="i-lucide-shield-alert"
    title="Your role no longer allows this page"
    description="The bunker refused this request. Your role may have changed since you opened the page."
    :actions="[{ label: 'Go to the dashboard', to: '/', color: 'neutral', variant: 'outline' }]"
  />
</template>
