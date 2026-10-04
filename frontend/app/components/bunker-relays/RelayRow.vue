<script setup lang="ts">
// One of the bunker's own relays (#78): its address, whether the bunker is connected to it, and,
// when the list can be changed here, Remove, or Undo for a removal not yet saved.
const props = defineProps<{
  url: string
  /** Connected now; unknown for a relay not saved yet. */
  connected?: boolean
  /** A change not saved yet. */
  pending?: 'add' | 'remove'
  /** NIP46_RELAYS decides: no actions. */
  readOnly?: boolean
}>()
const emit = defineEmits<{ remove: [], undo: [] }>()

const host = computed(() => props.url.replace(/^wss?:\/\//u, ''))
const status = computed(() => props.connected === undefined
  ? undefined
  : props.connected ? 'Connected' : 'Not connected')
</script>

<template>
  <li
    class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center"
    :data-relay="url"
    :data-pending="pending"
  >
    <div class="flex min-w-0 flex-1 items-center gap-2">
      <span
        v-if="status"
        class="size-2 shrink-0 rounded-full"
        :class="connected ? 'bg-success' : 'bg-error'"
        :title="status"
        :aria-label="status"
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
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-3">
      <span
        v-if="status"
        class="text-xs text-muted"
      >{{ status }}</span>
      <UBadge
        v-if="pending"
        size="sm"
        variant="subtle"
        :color="pending === 'remove' ? 'warning' : 'success'"
      >
        {{ pending === 'remove' ? 'Will remove' : 'New' }}
      </UBadge>
      <template v-if="!readOnly">
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
      </template>
    </div>
  </li>
</template>
