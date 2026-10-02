<script setup lang="ts">
// One relay in the member's list (#33): its address, Read and Write, whether it answered when the
// list was read, and Remove, or Undo for a staged change.
import type { Markers } from '~/utils/relay-list'
import { isInsecureRemote } from '~/utils/relay-url'

const props = defineProps<{
  url: string
  read: boolean
  write: boolean
  /** A staged change to this relay, if any. */
  pending?: 'add' | 'remove' | 'markers'
  /** Whether it answered when the list was read; unknown for a relay not asked. */
  reachable?: boolean
}>()
const emit = defineEmits<{ markers: [Markers], remove: [], undo: [] }>()

const host = computed(() => props.url.replace(/^wss?:\/\//u, '').replace(/\/$/u, ''))
const insecure = computed(() => isInsecureRemote(props.url))
const badge = computed(() => ({ add: 'New', remove: 'Will remove', markers: 'Changed' } as const)[props.pending ?? 'markers'])
</script>

<template>
  <li
    class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center"
    :data-relay="url"
    :data-pending="pending"
  >
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <span
        v-if="reachable !== undefined"
        class="size-2 shrink-0 rounded-full"
        :class="reachable ? 'bg-success' : 'bg-error'"
        :title="reachable ? 'Answered when your list was read' : 'Did not answer when your list was read'"
        :aria-label="reachable ? 'Answered' : 'Did not answer'"
        role="img"
      />
      <div
        class="min-w-0"
        :class="pending === 'remove' ? 'opacity-60 line-through' : ''"
      >
        <p class="text-sm font-medium truncate">
          {{ host }}
        </p>
        <p class="text-xs text-muted font-mono truncate">
          {{ url }}
        </p>
        <p
          v-if="insecure"
          class="text-xs text-warning"
        >
          Unencrypted (ws://): anyone on the network path can read this traffic.
        </p>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-3">
      <UCheckbox
        :model-value="read"
        label="Read"
        :disabled="pending === 'remove'"
        @update:model-value="value => emit('markers', { read: value === true, write })"
      />
      <UCheckbox
        :model-value="write"
        label="Write"
        :disabled="pending === 'remove'"
        @update:model-value="value => emit('markers', { read, write: value === true })"
      />
      <UBadge
        v-if="pending"
        size="sm"
        variant="subtle"
        :color="pending === 'remove' ? 'warning' : 'success'"
      >
        {{ badge }}
      </UBadge>
      <UButton
        v-if="pending === 'remove'"
        size="sm"
        color="neutral"
        variant="ghost"
        icon="i-lucide-undo-2"
        :aria-label="`Undo removing ${host}`"
        @click="emit('undo')"
      >
        Undo
      </UButton>
      <UButton
        v-else
        size="sm"
        color="neutral"
        variant="ghost"
        icon="i-lucide-trash-2"
        :aria-label="`Remove ${host}`"
        @click="emit('remove')"
      >
        Remove
      </UButton>
    </div>
  </li>
</template>
