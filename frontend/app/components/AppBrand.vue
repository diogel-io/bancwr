<script setup lang="ts">
import { useFetch, computed } from '#imports'
import type { BunkerStatus } from '#shared/types/bunker'

// The product name and the running release (#124), laid out as Porwr's sidebar brand: the logo,
// then the name with `V <version>` beneath it. Both images ship one release version, so this shows
// the bunker's, which is the one holding the keys. GET /api/version stays for operators.
defineProps<{ collapsed?: boolean }>()

// Its own key, so this header and the dashboard's status card never share a cache entry.
const { data: status } = await useFetch<BunkerStatus>('/api/bunker/status', { key: 'bunker-status-version' })

const version = computed(() => `V ${status.value?.version || 'unknown'}`)
const summary = computed(() => `Bancwr ${version.value}`)
</script>

<template>
  <!-- Collapsed, the sidebar has room for the logo only, so the name and version move into the
       tooltip and the link's accessible name. -->
  <UTooltip
    v-if="collapsed"
    :text="summary"
  >
    <NuxtLink
      to="/"
      :aria-label="summary"
      class="flex items-center"
    >
      <DiogelLogo size="md" />
    </NuxtLink>
  </UTooltip>

  <NuxtLink
    v-else
    to="/"
    class="flex items-center gap-3"
  >
    <!-- Porwr's size: the artwork has wide margins, so a smaller box reads as a small mark. -->
    <DiogelLogo size="lg" />
    <span class="min-w-0">
      <span class="block font-bold text-xl leading-tight">Bancwr</span>
      <span
        class="block text-xs text-muted tabular-nums"
        data-testid="app-version"
      >{{ version }}</span>
    </span>
  </NuxtLink>
</template>
