<script setup lang="ts">
// The log retention policy (#96), on the Config page for administrators: how long the signing log
// and the audit events are kept. Unset keeps everything. The bunker prunes at once when it is
// saved, then hourly, and records each change and each prune.
import type { LogRetention, LogRetentionResponse } from '#shared/types/bunker'
import { describeRetention, parseRetentionField } from '~/utils/logs'

const { data, error, refresh } = await useFetch<LogRetentionResponse>('/api/bunker/logs/retention', { key: 'log-retention' })

const days = ref('')
const rows = ref('')
function reset() {
  days.value = data.value?.max_age_days?.toString() ?? ''
  rows.value = data.value?.max_rows?.toString() ?? ''
}
watch(data, reset, { immediate: true })

const minDays = computed(() => data.value?.min_age_days ?? 30)
const minRows = computed(() => data.value?.min_rows ?? 10000)
const parsedDays = computed(() => parseRetentionField(days.value, minDays.value, 'days'))
const parsedRows = computed(() => parseRetentionField(rows.value, minRows.value, 'rows'))
const daysError = computed(() => 'error' in parsedDays.value ? parsedDays.value.error : undefined)
const rowsError = computed(() => 'error' in parsedRows.value ? parsedRows.value.error : undefined)
const draft = computed<LogRetention | undefined>(() => {
  if (!('value' in parsedDays.value) || !('value' in parsedRows.value)) return undefined
  return { max_age_days: parsedDays.value.value, max_rows: parsedRows.value.value }
})
const dirty = computed(() => !!data.value && !!draft.value
  && (draft.value.max_age_days !== data.value.max_age_days || draft.value.max_rows !== data.value.max_rows))

const saving = ref(false)
const saveError = ref<string>()
const savedNotice = ref(false)
watch([days, rows], () => {
  savedNotice.value = false
})

async function save() {
  if (!draft.value || !dirty.value || saving.value) return
  saving.value = true
  saveError.value = undefined
  try {
    data.value = await $fetch<LogRetentionResponse>('/api/bunker/logs/retention', { method: 'PUT', body: draft.value })
    savedNotice.value = true
  } catch (failure) {
    type Reply = { message?: string, data?: { message?: string } }
    const body = (failure as { data?: Reply }).data
    saveError.value = body?.data?.message ?? body?.message ?? 'The bunker did not save the policy. Try again.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <UCard
    class="max-w-2xl"
    data-testid="log-retention"
  >
    <template #header>
      <h2 class="font-semibold">
        Log retention
      </h2>
      <p class="text-sm text-muted">
        How long the signing log and the record of changes are kept, in the bunker and in its backups.
        Older entries are deleted when you save, then every hour. Export them first if you need them.
      </p>
    </template>

    <UAlert
      v-if="error"
      color="error"
      variant="subtle"
      title="Couldn't read the retention policy"
      :actions="[{ label: 'Try again', color: 'neutral', variant: 'outline', onClick: () => refresh() }]"
      data-testid="log-retention-error"
    />

    <form
      v-else-if="data"
      class="space-y-4"
      @submit.prevent="save"
    >
      <p
        class="text-sm"
        data-testid="log-retention-current"
      >
        {{ describeRetention(data) }}
      </p>

      <div class="flex flex-col gap-3 sm:flex-row sm:items-start">
        <UFormField
          label="Keep for (days)"
          :help="`Empty keeps entries indefinitely. At least ${minDays}.`"
          :error="daysError"
          class="flex-1"
        >
          <UInput
            v-model="days"
            inputmode="numeric"
            placeholder="Indefinitely"
            class="w-full"
            data-testid="log-retention-days"
          />
        </UFormField>
        <UFormField
          label="Keep at most (rows)"
          :help="`Empty sets no limit. At least ${minRows.toLocaleString('en-GB')}.`"
          :error="rowsError"
          class="flex-1"
        >
          <UInput
            v-model="rows"
            inputmode="numeric"
            placeholder="No limit"
            class="w-full"
            data-testid="log-retention-rows"
          />
        </UFormField>
      </div>

      <div class="flex items-center gap-3">
        <UButton
          type="submit"
          :disabled="!dirty"
          :loading="saving"
          data-testid="log-retention-save"
        >
          Save
        </UButton>
        <UButton
          v-if="dirty"
          color="neutral"
          variant="ghost"
          @click="reset"
        >
          Discard
        </UButton>
        <p
          v-if="savedNotice"
          class="text-sm text-success"
          data-testid="log-retention-saved"
        >
          Saved. Entries outside the policy have been deleted.
        </p>
        <p
          v-if="saveError"
          class="text-sm text-error"
          data-testid="log-retention-save-error"
        >
          {{ saveError }}
        </p>
      </div>
    </form>
  </UCard>
</template>
