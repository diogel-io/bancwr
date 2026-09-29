<script setup lang="ts">
import { useFetch, computed } from '#imports'
import type { BunkerStatus, VersionResponse } from '#shared/types/bunker'

// Which builds are running (#35). The bunker and the frontend are separate images, each with its
// own BANCWR_VERSION, so both are shown: a self-hoster who updates one and not the other should
// see that here rather than in a bug report.
defineProps<{ collapsed?: boolean }>()

// Its own key, so this footer and the dashboard's status card never share a cache entry.
const { data: status } = await useFetch<BunkerStatus>('/api/bunker/status', { key: 'bunker-status-version' })
const { data: frontend } = await useFetch<VersionResponse>('/api/version', { key: 'frontend-version' })

const bunkerVersion = computed(() => status.value?.version || 'unknown')
const frontendVersion = computed(() => frontend.value?.version || 'unknown')

// Only a difference between two known versions is a mismatch; an unreachable bunker is not one.
const mismatch = computed(() =>
  !!status.value?.version && !!frontend.value?.version && status.value.version !== frontend.value.version
)

const summary = computed(() => [
  `Bunker ${bunkerVersion.value}`,
  `Frontend ${frontendVersion.value}`,
  ...(mismatch.value ? ['Bunker and frontend versions differ'] : [])
].join(', '))
</script>

<template>
  <!-- Collapsed, the sidebar has room for an icon only, so the versions move into the tooltip and
       the accessible name. -->
  <UTooltip
    v-if="collapsed"
    :text="summary"
  >
    <div
      role="group"
      tabindex="0"
      :aria-label="summary"
      class="flex justify-center"
    >
      <UIcon
        :name="mismatch ? 'i-lucide-triangle-alert' : 'i-lucide-tag'"
        :class="mismatch ? 'text-warning' : 'text-muted'"
        class="size-4"
      />
    </div>
  </UTooltip>

  <div
    v-else
    role="group"
    :aria-label="summary"
    class="text-xs text-muted tabular-nums space-y-0.5"
  >
    <p>Bunker {{ bunkerVersion }}</p>
    <p>Frontend {{ frontendVersion }}</p>
    <!-- Only the icon is amber: amber text on the light theme falls short of contrast. -->
    <p
      v-if="mismatch"
      class="flex items-center gap-1 text-default"
    >
      <UIcon
        name="i-lucide-triangle-alert"
        class="size-3.5 shrink-0 text-warning"
      />
      Bunker and frontend versions differ
    </p>
  </div>
</template>
