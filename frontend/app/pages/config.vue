<script setup lang="ts">
import { useFetch } from '#imports'
import type { ConfigResponse } from '#shared/types/bunker'
import { isForbidden } from '~/utils/access'

// The key is read-only (#42). It is encrypted at rest and imported once with `bunker import-key`
// (#94); the API does not accept a key, and never returns one or says where it is stored. Below
// it, the bunker's own relays (#78), which an administrator can change unless NIP46_RELAYS sets
// them.
const { data: config, error } = await useFetch<ConfigResponse>('/api/bunker/config')
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
                Key Storage
              </dt>
              <dd
                class="mt-1 text-sm"
                data-testid="key-storage"
              >
                {{ config?.key_storage === 'encrypted' ? 'Encrypted at rest' : '' }}
              </dd>
            </div>
          </dl>

          <template #footer>
            <p class="text-sm text-muted">
              The signing key is encrypted under the root credential in
              <code>BANCWR_ROOT_KEY_FILE</code>. It is imported once with <code>bunker import-key</code>;
              see "Protecting the bunker key" in <code>docs/operating.md</code>.
            </p>
          </template>
        </UCard>

        <!-- The bunker's own NIP-46 relays (#78): editable unless NIP46_RELAYS sets them. -->
        <BunkerRelaysSection />
      </div>
    </template>
  </UDashboardPanel>
</template>
