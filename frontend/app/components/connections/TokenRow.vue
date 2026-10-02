<script setup lang="ts">
// An issued token not yet used (#31): its label, for whom, what it allows, when it expires.
import type { Nip46Token } from '#shared/types/bunker'
import { kindLabel, relativeTime } from '~/utils/connections'

defineProps<{ token: Nip46Token, forName: string }>()
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
        For {{ forName }} · may sign {{ token.kinds.map(kindLabel).join(', ') }} · expires {{ relativeTime(token.expires_at) }}
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
