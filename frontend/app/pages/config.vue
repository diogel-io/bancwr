<script setup lang="ts">
import { useFetch, computed } from '#imports'
import type { ConfigResponse } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

// The key is read-only (#42). The signing key is set with BUNKER_NSEC_FILE or BUNKER_NSEC when the
// bunker starts; the API does not accept a key, and never returns one. Below it, the bunker's own
// relays (#78), which an administrator can change unless NIP46_RELAYS sets them.
const { data: config, error } = await useFetch<ConfigResponse>('/api/bunker/config')

const keySource = computed(() =>
  config.value?.nsec_file
    ? { label: 'File', value: config.value.nsec_file }
    : { label: 'Environment variable', value: 'BUNKER_NSEC' }
)
</script>

<template>
  <UDashboardPanel id="config">
    <template #header>
      <AppNavbar title="Bunker Configuration" />
    </template>

    <template #body>
      <ForbiddenNotice v-if="isForbidden(error)" />
      <div
        v-else
        class="space-y-6"
      >
        <UCard class="max-w-2xl">
          <dl class="space-y-6">
            <div>
              <dt class="text-sm font-medium">
                Current Pubkey
              </dt>
              <dd class="mt-1">
                <code class="text-sm break-all">{{ config?.pubkey ?? '' }}</code>
              </dd>
              <p class="text-sm text-muted mt-1">
                This is the public key for this bunker.
              </p>
            </div>

            <div>
              <dt class="text-sm font-medium">
                Key Source
              </dt>
              <dd class="mt-1 text-sm">
                {{ keySource.label }}: <code class="break-all">{{ keySource.value }}</code>
              </dd>
            </div>
          </dl>

          <template #footer>
            <p class="text-sm text-muted">
              To change the signing key, set <code>BUNKER_NSEC_FILE</code> (recommended) or
              <code>BUNKER_NSEC</code> for the bunker service in <code>compose.yaml</code> and restart it.
            </p>
          </template>
        </UCard>

        <!-- The bunker's own NIP-46 relays (#78): editable unless NIP46_RELAYS sets them. -->
        <BunkerRelaysSection />
      </div>
    </template>
  </UDashboardPanel>
</template>
