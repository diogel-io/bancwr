<script setup lang="ts">
// An issued token not yet used (#31): its label, for whom, what it allows, when it expires.
import type { Nip46Token } from '#shared/types/bunker'
import { kindLabel, lifetimeLabel, methodLabel, rateLimitLabel, relativeTime } from '~/utils/connections'

defineProps<{ token: Nip46Token, forName: string, identityLabel?: string }>()
const emit = defineEmits<{ revoke: [] }>()
</script>

<template>
  <li
    class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center"
    :data-token="token.id"
  >
    <div class="min-w-0 flex-1">
      <p class="text-sm font-medium break-all">
        {{ token.label }}
      </p>
      <p class="text-xs text-muted">
        For {{ forName }}
        <template v-if="identityLabel">
          · <span data-testid="token-identity">signs as {{ identityLabel }}</span>
        </template>
        <template v-if="token.kinds.length">
          · may sign {{ token.kinds.map(kindLabel).join(', ') }}
        </template>
        <template v-if="token.methods?.length">
          · may also: {{ token.methods.map(methodLabel).join(', ') }}
        </template>
        <template v-if="token.connection_lifetime_hours">
          · connection lasts {{ lifetimeLabel(token.connection_lifetime_hours) }}
        </template>
        <template v-if="token.approval_kinds?.length">
          · <span data-testid="token-approval">needs approval: {{ token.approval_kinds.map(kindLabel).join(', ') }}</span>
        </template>
        <template v-if="token.rate_per_minute">
          · <span data-testid="token-rate-limit">limited to {{ rateLimitLabel(token.rate_per_minute, token.rate_burst) }}</span>
        </template>
        · expires {{ relativeTime(token.expires_at) }}
      </p>
    </div>
    <UButton
      size="sm"
      color="neutral"
      variant="ghost"
      icon="i-lucide-x"
      :aria-label="`Revoke token ${token.label}`"
      @click="emit('revoke')"
    >
      Revoke
    </UButton>
  </li>
</template>
