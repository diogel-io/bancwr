<script setup lang="ts">
// The bunker's health, top left of every page's header (#28): red, yellow or green, with a text
// label so colour is never the only carrier. Click for each check's detail (#27).
import { subscribeToBunkerHealth } from '~/composables/useBunkerHealth'
import type { IndicatorState } from '~/composables/useBunkerHealth'

const { health, refresh, ensure } = useBunkerHealth()

// Server rendering fetches once, so a full page load arrives with a known state.
if (import.meta.server) await ensure()

const STATES: Record<IndicatorState, { label: string, dot: string }> = {
  unknown: { label: 'Checking…', dot: 'bg-neutral-400 dark:bg-neutral-500' },
  healthy: { label: 'Healthy', dot: 'bg-success' },
  degraded: { label: 'Degraded', dot: 'bg-warning' },
  unhealthy: { label: 'Down', dot: 'bg-error' }
}
const current = computed(() => STATES[health.value.state])

// "12 s ago", ticking only while the popover is open.
const open = ref(false)
const now = ref(Date.now())
let tick: ReturnType<typeof setInterval> | undefined
watch(open, (isOpen) => {
  if (tick) clearInterval(tick)
  tick = undefined
  if (isOpen) {
    now.value = Date.now()
    tick = setInterval(() => (now.value = Date.now()), 1000)
  }
})
const checkedAgo = computed(() => {
  if (!health.value.checkedAt) return 'Not checked yet'
  const seconds = Math.max(0, Math.round((now.value - health.value.checkedAt) / 1000))
  return seconds < 60 ? `Last checked ${seconds} s ago` : `Last checked ${Math.round(seconds / 60)} min ago`
})

const checking = ref(false)
async function checkNow() {
  checking.value = true
  try {
    await refresh()
    now.value = Date.now()
  } finally {
    checking.value = false
  }
}

let unsubscribe: (() => void) | undefined
onMounted(() => {
  unsubscribe = subscribeToBunkerHealth(refresh)
  // A client-only mount with nothing known yet asks now rather than in 30 s.
  void ensure()
})
onBeforeUnmount(() => {
  unsubscribe?.()
  if (tick) clearInterval(tick)
})
</script>

<template>
  <UPopover v-model:open="open">
    <UButton
      data-testid="bunker-health"
      :data-state="health.state"
      color="neutral"
      variant="ghost"
      size="sm"
      :aria-label="`Bunker health: ${current.label}. Show details`"
    >
      <span
        class="size-2.5 rounded-full shrink-0"
        :class="current.dot"
        aria-hidden="true"
      />
      <span class="text-sm">{{ current.label }}</span>
    </UButton>

    <template #content>
      <div
        data-testid="bunker-health-detail"
        class="w-80 max-w-[calc(100vw-2rem)] p-4 space-y-4"
      >
        <p class="text-sm font-semibold">
          Bunker health: {{ current.label }}
        </p>
        <HealthChecks
          v-if="health.checks.length"
          :checks="health.checks"
        />
        <div class="flex items-center justify-between gap-3">
          <span class="text-xs text-muted">{{ checkedAgo }}</span>
          <UButton
            size="xs"
            color="neutral"
            variant="outline"
            icon="i-lucide-refresh-cw"
            :loading="checking"
            @click="checkNow"
          >
            Check now
          </UButton>
        </div>
      </div>
    </template>
  </UPopover>

  <!-- Announces a change of state, not every poll: the text only changes when the state does. -->
  <span
    role="status"
    class="sr-only"
  >Bunker health: {{ current.label }}</span>
</template>
