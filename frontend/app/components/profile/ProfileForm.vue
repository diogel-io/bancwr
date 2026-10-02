<script setup lang="ts">
// The profile's fields (#30), Porwr's set: names, about, website, images, NIP-05, Lightning
// address, bot, and a birthday in three parts (NIP-24).
import type { ImageKind } from '~/utils/image'
import type { ProfileForm, TextField } from '~/utils/profile'

defineProps<{
  pubkey: string
  errors: Partial<Record<TextField | 'birthday', string>>
  upload: (file: File, kind: ImageKind) => Promise<string>
}>()
const form = defineModel<ProfileForm>({ required: true })
</script>

<template>
  <div class="space-y-6">
    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <UFormField
        label="Name"
        help="Short handle, without spaces."
      >
        <UInput
          v-model="form.name"
          class="w-full"
          autocomplete="off"
        />
      </UFormField>
      <UFormField
        label="Display name"
        help="How your name is shown."
      >
        <UInput
          v-model="form.display_name"
          class="w-full"
          autocomplete="off"
        />
      </UFormField>
    </div>

    <UFormField label="About">
      <UTextarea
        v-model="form.about"
        class="w-full"
        :rows="4"
        autoresize
      />
    </UFormField>

    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <UFormField
        label="Website"
        :error="errors.website"
      >
        <UInput
          v-model="form.website"
          class="w-full"
          placeholder="https://…"
          inputmode="url"
        />
      </UFormField>
      <UFormField
        label="Lightning address"
        help="For tips (lud16)."
        :error="errors.lud16"
      >
        <UInput
          v-model="form.lud16"
          class="w-full"
          placeholder="name@wallet.example"
          autocomplete="off"
        />
      </UFormField>
    </div>

    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <ProfileImageField
        v-model="form.picture"
        label="Picture"
        kind="picture"
        :upload="upload"
        :error="errors.picture"
      />
      <ProfileImageField
        v-model="form.banner"
        label="Banner"
        kind="banner"
        :upload="upload"
        :error="errors.banner"
      />
    </div>

    <ProfileIdentityVerification
      v-model="form.nip05"
      :pubkey="pubkey"
    />

    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <UFormField
        label="Birthday"
        help="Any part can be left empty."
        :error="errors.birthday"
      >
        <div class="grid grid-cols-3 gap-2">
          <UInput
            v-model="form.birthday.year"
            placeholder="Year"
            inputmode="numeric"
            aria-label="Birthday year"
          />
          <UInput
            v-model="form.birthday.month"
            placeholder="Month"
            inputmode="numeric"
            aria-label="Birthday month"
          />
          <UInput
            v-model="form.birthday.day"
            placeholder="Day"
            inputmode="numeric"
            aria-label="Birthday day"
          />
        </div>
      </UFormField>
      <UFormField
        label="Automated account"
        help="Tell clients this key is run by a program."
      >
        <USwitch
          v-model="form.bot"
          label="This is a bot"
        />
      </UFormField>
    </div>
  </div>
</template>
