<script setup lang="ts">
// One followed key (#32): picture, name and npub, with Remove, or Undo for a staged change.
import { npubEncode } from 'nostr-tools/nip19'
import { displayName, type Follow, type FollowProfile } from '~/utils/follows'
import { NoReferrerImg } from '~/utils/no-referrer-img'

// Third-party pictures load with no referrer, the policy set before src (#75).
const avatarAs = { img: NoReferrerImg }
const props = defineProps<{
  follow: Follow
  profile?: FollowProfile
  /** A staged change to this row, if any. */
  pending?: 'add' | 'remove'
}>()
const emit = defineEmits<{ remove: [], undo: [] }>()

const npub = computed(() => npubEncode(props.follow.pubkey))
const name = computed(() => displayName(props.follow, props.profile))
</script>

<template>
  <li
    class="flex items-center gap-3 py-3"
    :data-pubkey="follow.pubkey"
    :data-pending="pending"
  >
    <UAvatar
      :src="profile?.picture"
      :alt="name"
      :as="avatarAs"
      size="md"
    />
    <div
      class="min-w-0 flex-1"
      :class="pending === 'remove' ? 'opacity-60 line-through' : ''"
    >
      <p class="text-sm font-medium truncate">
        {{ name }}
      </p>
      <p
        class="text-xs text-muted font-mono truncate"
        :title="npub"
      >
        {{ npub }}
      </p>
      <p
        v-if="profile?.nip05"
        class="text-xs text-muted truncate"
      >
        {{ profile.nip05 }}
      </p>
    </div>
    <UBadge
      v-if="pending"
      size="sm"
      variant="subtle"
      :color="pending === 'add' ? 'success' : 'warning'"
    >
      {{ pending === 'add' ? 'Will follow' : 'Will unfollow' }}
    </UBadge>
    <UButton
      v-if="pending"
      size="sm"
      color="neutral"
      variant="ghost"
      icon="i-lucide-undo-2"
      :aria-label="`Undo for ${name}`"
      @click="emit('undo')"
    >
      Undo
    </UButton>
    <UButton
      v-else
      size="sm"
      color="neutral"
      variant="ghost"
      icon="i-lucide-user-minus"
      :aria-label="`Unfollow ${name}`"
      @click="emit('remove')"
    >
      Remove
    </UButton>
  </li>
</template>
