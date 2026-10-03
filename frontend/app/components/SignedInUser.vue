<script setup lang="ts">
// Who is signed in, and signing out, in the sidebar footer (#11), with the key's avatar and name
// from its profile (#72). The full npub stays in the title and the accessible name.
import { ROLE_LABELS } from '#shared/types/bunker'
import { NoReferrerImg } from '~/utils/no-referrer-img'

defineProps<{ collapsed?: boolean }>()

const auth = useAuth()
const { pubkey, profile, load } = useSignedInProfile()
const user = computed(() => auth.state.value.status === 'signed-in' ? auth.state.value : undefined)
const short = computed(() => user.value ? `${user.value.npub.slice(0, 12)}…${user.value.npub.slice(-4)}` : '')
const summary = computed(() => user.value ? `Signed in as ${user.value.npub}, ${ROLE_LABELS[user.value.role]}` : '')
const avatarAs = { img: NoReferrerImg }
const label = computed(() => profile.value?.name ?? short.value)

// The layout stays mounted across navigation, so this looks up once per signed-in key.
onMounted(() => {
  void load()
  watch(pubkey, () => void load())
})
</script>

<template>
  <div
    v-if="user"
    :class="collapsed ? 'flex flex-col items-center gap-2' : undefined"
  >
    <!-- The picture is a third-party URL: no referrer (NoReferrerImg), so its host isn't told which Bancwr shows it.
         No picture, one still loading, or one that fails to load all show the user icon. -->
    <UTooltip
      v-if="collapsed"
      :text="summary"
    >
      <UAvatar
        :src="profile?.picture"
        :alt="label"
        icon="i-lucide-user"
        size="md"
        :as="avatarAs"
        data-testid="user-avatar"
      />
    </UTooltip>
    <UTooltip
      v-if="collapsed"
      :text="summary"
    >
      <UButton
        icon="i-lucide-log-out"
        color="neutral"
        variant="ghost"
        :aria-label="`Sign out. ${summary}`"
        @click="auth.signOut()"
      />
    </UTooltip>
    <div
      v-else
      class="flex items-center gap-2"
      role="group"
      :aria-label="summary"
    >
      <UAvatar
        :src="profile?.picture"
        :alt="label"
        icon="i-lucide-user"
        size="md"
        :as="avatarAs"
        data-testid="user-avatar"
      />
      <div class="min-w-0 flex-1 text-xs">
        <p
          class="truncate"
          :class="profile?.name ? 'font-medium' : 'font-mono'"
          :title="user.npub"
          data-testid="user-name"
        >
          {{ label }}
        </p>
        <p class="text-muted">
          {{ ROLE_LABELS[user.role] }}
        </p>
      </div>
      <UButton
        icon="i-lucide-log-out"
        color="neutral"
        variant="ghost"
        size="sm"
        aria-label="Sign out"
        @click="auth.signOut()"
      />
    </div>
  </div>
</template>
