<script setup lang="ts">
import { useFetch, ref, computed, watch } from '#imports'
import type { LogEntry, LogExportFormat } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'
import { LOG_PAGE_SIZE, defaultExportRange, exportRangeError, exportUrl, olderLogsUrl } from '~/utils/logs'

// The whole signing log, a page at a time (#96): the newest 100, then older pages on request.
const { data: logs, error } = await useFetch<LogEntry[]>('/api/bunker/logs', { key: 'logs-full' })

const older = ref<LogEntry[]>([])
const rows = computed(() => [...(logs.value ?? []), ...older.value])
/** A short page means the log has no more rows. */
const lastPageLength = ref<number>()
watch(logs, () => {
  older.value = []
  lastPageLength.value = undefined
})
const hasMore = computed(() => (lastPageLength.value ?? logs.value?.length ?? 0) >= LOG_PAGE_SIZE)
const loading = ref(false)
const loadError = ref<string>()

async function loadOlder() {
  if (loading.value || !hasMore.value) return
  loading.value = true
  loadError.value = undefined
  try {
    const page = await $fetch<LogEntry[]>(olderLogsUrl(rows.value))
    older.value = [...older.value, ...page]
    lastPageLength.value = page.length
  } catch {
    loadError.value = 'The bunker did not return older entries. Try again.'
  } finally {
    loading.value = false
  }
}

// The export (#96): every signature and audit event in a range of UTC dates, downloaded as a file.
const range = defaultExportRange(new Date())
const from = ref(range.from)
const to = ref(range.to)
const format = ref<LogExportFormat>('csv')
const formats = [
  { label: 'CSV', value: 'csv' },
  { label: 'JSON Lines', value: 'jsonl' }
]
const rangeError = computed(() => exportRangeError(from.value, to.value))
const downloadUrl = computed(() => rangeError.value ? undefined : exportUrl(format.value, from.value, to.value))
</script>

<template>
  <UDashboardPanel id="logs">
    <template #header>
      <AppNavbar title="Signing Activity Log" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <div
        v-else
        class="space-y-6"
      >
        <UCard data-testid="log-export">
          <template #header>
            <h2 class="font-semibold">
              Export
            </h2>
            <p class="text-sm text-muted">
              Every signature and every recorded change (tokens, connections, members, relays and
              retention) between two dates, in UTC, both included. Event content is never recorded.
              The export itself is recorded.
            </p>
          </template>

          <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
            <UFormField label="From">
              <UInput
                v-model="from"
                type="date"
                data-testid="log-export-from"
              />
            </UFormField>
            <UFormField label="To">
              <UInput
                v-model="to"
                type="date"
                data-testid="log-export-to"
              />
            </UFormField>
            <UFormField label="Format">
              <USelect
                v-model="format"
                :items="formats"
                class="w-36"
                data-testid="log-export-format"
              />
            </UFormField>
            <UButton
              :to="downloadUrl"
              :disabled="!downloadUrl"
              external
              download
              icon="i-lucide-download"
              data-testid="log-export-download"
            >
              Download
            </UButton>
          </div>
          <p
            v-if="rangeError"
            class="mt-2 text-sm text-error"
            data-testid="log-export-error"
          >
            {{ rangeError }}
          </p>
        </UCard>

        <UCard>
          <ActivityLog :rows="rows" />
          <template
            v-if="hasMore || loadError"
            #footer
          >
            <div class="flex items-center gap-3">
              <UButton
                v-if="hasMore"
                color="neutral"
                variant="outline"
                :loading="loading"
                data-testid="log-load-older"
                @click="loadOlder"
              >
                Load older
              </UButton>
              <p
                v-if="loadError"
                class="text-sm text-error"
              >
                {{ loadError }}
              </p>
            </div>
          </template>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>
