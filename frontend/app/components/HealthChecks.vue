<script setup lang="ts">
// Each health check and what needs attention (#28), for the navbar popover and the dashboard card.
import type { CheckStatus, HealthCheck } from '#shared/types/bunker'

defineProps<{ checks: HealthCheck[] }>()

const NAMES: Record<string, string> = { signer: 'Signer', database: 'Database', relays: 'Relays', administrator: 'Administrator', bunker: 'Bunker' }
const BADGES: Record<CheckStatus, { label: string, color: 'success' | 'warning' | 'error' | 'neutral' }> = {
  pass: { label: 'OK', color: 'success' },
  warn: { label: 'Needs attention', color: 'warning' },
  fail: { label: 'Failing', color: 'error' },
  disabled: { label: 'Off', color: 'neutral' }
}
</script>

<template>
  <ul
    data-testid="health-checks"
    class="space-y-3"
  >
    <li
      v-for="check in checks"
      :key="check.name"
      :data-check="check.name"
      class="space-y-1"
    >
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm font-medium">{{ NAMES[check.name] ?? check.name }}</span>
        <UBadge
          :color="BADGES[check.status]?.color ?? 'neutral'"
          variant="subtle"
          size="sm"
        >
          {{ BADGES[check.status]?.label ?? check.status }}
        </UBadge>
      </div>
      <p class="text-xs text-muted break-words">
        {{ check.detail }}
      </p>
      <ul
        v-if="check.relays?.length"
        class="text-xs space-y-0.5"
      >
        <li
          v-for="relay in check.relays"
          :key="relay.url"
          class="flex items-center gap-2"
        >
          <UIcon
            :name="relay.connected ? 'i-lucide-check' : 'i-lucide-x'"
            :class="relay.connected ? 'text-success' : 'text-error'"
            class="size-3.5 shrink-0"
            aria-hidden="true"
          />
          <span class="font-mono break-all">{{ relay.url }}</span>
          <span class="text-muted">{{ relay.connected ? 'connected' : 'not connected' }}</span>
        </li>
      </ul>
    </li>
  </ul>
</template>
