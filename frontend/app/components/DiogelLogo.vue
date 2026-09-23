<script setup lang="ts">
import { computed } from 'vue'
import darkLogoUrl from '~/assets/images/dark/diogel.svg'
import lightLogoUrl from '~/assets/images/light/diogel.svg'

const props = withDefaults(defineProps<{ size?: 'sm' | 'md' | 'lg' | 'xl' }>(), {
  size: 'md'
})

const sizeClass = computed(() => ({
  sm: 'w-6 h-6',
  md: 'w-9 h-9',
  lg: 'w-[90px] h-[90px]',
  xl: 'w-[150px] h-[150px]'
}[props.size]))
</script>

<template>
  <!--
    The two marks are different artwork, not a recolour: the light one is drawn in #111827/#374151
    and the dark one in #ffffff/#6b7280, over a shared #f97316.

    Both are rendered and CSS picks one. Choosing the src from useColorMode() instead would mean
    the server renders a mark it cannot know is right, giving a hydration mismatch and a visible
    flash on first paint. The class swap is resolved before the page is interactive.
  -->
  <span :class="['inline-block shrink-0', sizeClass]">
    <img
      :src="lightLogoUrl"
      alt="Diogel"
      class="w-full h-full object-contain dark:hidden"
    >
    <img
      :src="darkLogoUrl"
      alt=""
      aria-hidden="true"
      class="w-full h-full object-contain hidden dark:block"
    >
  </span>
</template>
