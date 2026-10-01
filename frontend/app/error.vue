<script setup lang="ts">
// Every error page. A route the signed-in role may not open (#26) gets its own message, inside the
// default layout so the sidebar stays; anything else keeps the usual status and message.
import type { NuxtError } from '#app'
import { ROLE_LABELS } from '#shared/types/bunker'
import { pageTitle } from '~/utils/access'

const props = defineProps<{ error: NuxtError }>()

const status = computed(() => props.error.status ?? props.error.statusCode ?? 500)

// The data arrives as a string when the error was raised during server rendering.
const data = computed<{ reason?: string, path?: string }>(() => {
  const raw = props.error.data as unknown
  if (typeof raw === 'string') {
    try {
      return JSON.parse(raw)
    } catch {
      return {}
    }
  }
  return (raw as { reason?: string, path?: string }) ?? {}
})

const forbidden = computed(() => status.value === 403 && data.value.reason === 'forbidden_route')
const page = computed(() => pageTitle(data.value.path ?? useRoute().path))

const auth = useAuth()
const role = computed(() => auth.state.value.status === 'signed-in' ? ROLE_LABELS[auth.state.value.role] : undefined)

const message = computed(() => props.error.statusText ?? props.error.statusMessage ?? props.error.message)

function toDashboard() {
  return clearError({ redirect: '/' })
}
</script>

<template>
  <UApp>
    <NuxtLayout>
      <UDashboardPanel id="error">
        <template #header>
          <AppNavbar :title="forbidden ? 'No access' : `Error ${status}`" />
        </template>

        <template #body>
          <UCard class="max-w-2xl">
            <div
              v-if="forbidden"
              data-testid="permission-denied"
              class="space-y-2"
            >
              <h1 class="text-lg font-semibold">
                You don't have access to {{ page }}
              </h1>
              <p class="text-sm text-muted">
                <template v-if="role">
                  You are signed in as {{ role }}, which cannot open this page.
                </template>
                Ask the vault administrator if you need it.
              </p>
            </div>
            <div
              v-else
              class="space-y-2"
            >
              <h1 class="text-lg font-semibold">
                {{ status }}
              </h1>
              <p class="text-sm text-muted">
                {{ message }}
              </p>
            </div>

            <template #footer>
              <UButton
                color="neutral"
                variant="outline"
                icon="i-lucide-layout-dashboard"
                @click="toDashboard"
              >
                Go to the dashboard
              </UButton>
            </template>
          </UCard>
        </template>
      </UDashboardPanel>
    </NuxtLayout>
  </UApp>
</template>
