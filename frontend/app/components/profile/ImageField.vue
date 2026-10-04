<script setup lang="ts">
// A profile image (#30): an address, or a file uploaded to Blossom, with a preview. Uploading only
// fills the field; nothing is published until the profile is saved.
import { checkImage, type ImageKind } from '~/utils/image'
import { isHttpUrl } from '~/utils/profile'
import { NoReferrerImg } from '~/utils/no-referrer-img'

const props = defineProps<{
  label: string
  kind: ImageKind
  /** Uploads the file and returns its URL; throws with a message to show. */
  upload: (file: File, kind: ImageKind) => Promise<string>
  error?: string
}>()
const url = defineModel<string>({ required: true })

const input = ref<HTMLInputElement>()
const uploading = ref(false)
const uploadError = ref<string>()
const broken = ref(false)

const preview = computed(() => isHttpUrl(url.value.trim()) && !broken.value ? url.value.trim() : undefined)
watch(url, () => (broken.value = false))

async function chosen(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0]
  ;(event.target as HTMLInputElement).value = ''
  if (!file) return
  uploadError.value = checkImage(file)
  if (uploadError.value) return
  uploading.value = true
  try {
    url.value = await props.upload(file, props.kind)
  } catch (failure) {
    uploadError.value = failure instanceof Error ? failure.message : 'The upload failed.'
  } finally {
    uploading.value = false
  }
}
</script>

<template>
  <UFormField
    :label="label"
    :error="error || uploadError"
    :help="kind === 'banner' ? 'Wide image, shown at the top of your profile.' : 'Square image, shown as your avatar.'"
  >
    <div class="space-y-2">
      <div
        v-if="preview"
        class="overflow-hidden rounded-md border border-default bg-elevated"
        :class="kind === 'banner' ? 'aspect-[3/1] w-full' : 'size-24'"
      >
        <!-- No referrer, the policy set before src (#75). -->
        <NoReferrerImg
          :src="preview"
          :alt="`${label} preview`"
          class="size-full object-cover"
          @error="broken = true"
        />
      </div>
      <UInput
        v-model="url"
        class="w-full"
        placeholder="https://…"
        :disabled="uploading"
        :aria-label="`${label} address`"
      />
      <div class="flex flex-wrap gap-2">
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          icon="i-lucide-upload"
          :loading="uploading"
          @click="input?.click()"
        >
          {{ uploading ? 'Uploading…' : 'Upload image' }}
        </UButton>
        <UButton
          v-if="url"
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-x"
          :disabled="uploading"
          @click="url = ''"
        >
          Remove
        </UButton>
        <input
          ref="input"
          type="file"
          class="hidden"
          accept="image/png,image/jpeg,image/webp,image/gif"
          :data-testid="`${kind}-file`"
          @change="chosen"
        >
      </div>
      <p class="text-xs text-muted">
        PNG, JPEG or WebP are resized and stripped of camera and location data; GIFs are uploaded as they are.
      </p>
    </div>
  </UFormField>
</template>
