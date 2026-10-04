<script setup lang="ts">
// How the profile will look (#30), from the form as it is edited.
import { isHttpUrl, type ProfileForm } from '~/utils/profile'
import { NoReferrerImg } from '~/utils/no-referrer-img'

const props = defineProps<{ form: ProfileForm, npub: string }>()

// Third-party pictures load with no referrer, the policy set before src (#75).
const avatarAs = { img: NoReferrerImg }
const image = (value: string) => isHttpUrl(value.trim()) ? value.trim() : undefined
const name = computed(() => props.form.display_name.trim() || props.form.name.trim() || 'Unnamed')
</script>

<template>
  <UCard
    :ui="{ body: 'p-0 sm:p-0' }"
    data-testid="profile-preview"
  >
    <div class="aspect-[3/1] w-full bg-elevated">
      <NoReferrerImg
        v-if="image(form.banner)"
        :src="image(form.banner)"
        alt=""
        class="size-full object-cover"
      />
    </div>
    <div class="space-y-3 p-4">
      <UAvatar
        :src="image(form.picture)"
        :alt="name"
        :as="avatarAs"
        size="3xl"
        class="-mt-12 ring-4 ring-default"
      />
      <div>
        <p class="text-lg font-semibold break-words">
          {{ name }}
          <UBadge
            v-if="form.bot"
            size="sm"
            color="neutral"
            variant="subtle"
          >
            Bot
          </UBadge>
        </p>
        <p
          v-if="form.name.trim() && form.display_name.trim()"
          class="text-sm text-muted"
        >
          @{{ form.name.trim() }}
        </p>
        <p class="text-xs text-muted font-mono break-all">
          {{ npub }}
        </p>
      </div>
      <p
        v-if="form.about.trim()"
        class="text-sm whitespace-pre-line break-words"
      >
        {{ form.about }}
      </p>
      <ul class="space-y-1 text-sm">
        <li
          v-if="form.nip05.trim()"
          class="flex items-center gap-2"
        >
          <UIcon
            name="i-lucide-at-sign"
            class="size-4 shrink-0 text-muted"
            aria-hidden="true"
          />
          <span class="break-all">{{ form.nip05.trim() }}</span>
        </li>
        <li
          v-if="image(form.website)"
          class="flex items-center gap-2"
        >
          <UIcon
            name="i-lucide-link"
            class="size-4 shrink-0 text-muted"
            aria-hidden="true"
          />
          <span class="break-all">{{ form.website.trim() }}</span>
        </li>
        <li
          v-if="form.lud16.trim()"
          class="flex items-center gap-2"
        >
          <UIcon
            name="i-lucide-zap"
            class="size-4 shrink-0 text-muted"
            aria-hidden="true"
          />
          <span class="break-all">{{ form.lud16.trim() }}</span>
        </li>
      </ul>
    </div>
  </UCard>
</template>
