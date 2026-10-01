<script setup lang="ts">
// For the pages a visitor sees before (or instead of) the dashboard: sign-in and no-access (#11).
// No sidebar, and none of the default layout's bunker calls, which need a session.
</script>

<template>
  <div class="min-h-svh flex flex-col items-center justify-center gap-6 p-4 bg-default">
    <div class="flex items-center gap-2 font-bold text-xl">
      <DiogelLogo size="sm" />
      <span>Bancwr</span>
    </div>
    <UCard class="w-full max-w-md">
      <slot />
    </UCard>
    <UColorModeSwitch />
  </div>
</template>
