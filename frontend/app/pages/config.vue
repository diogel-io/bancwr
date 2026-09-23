<script setup lang="ts">
import { useFetch, reactive, ref, useToast } from '#imports'

const { data: config } = await useFetch('/api/bunker/config')

const state = reactive({
  nsec: config.value ? config.value['nsec'] : '',
  nsecFile: ''
})

const loading = ref(false)
const toast = useToast()

const saveConfig = async () => {
  loading.value = true
  try {
    await $fetch('/api/bunker/config', {
      method: 'POST',
      body: state
    })
    toast.add({ title: 'Config saved successfully', color: 'success' })
  } catch (e) {
    toast.add({ title: 'Failed to save config', color: 'error' })
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <UDashboardPanel id="config">
    <template #header>
      <UDashboardNavbar title="Bunker Configuration">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <UCard class="max-w-2xl">
        <div class="space-y-6">
          <UFormField
            label="Current Pubkey"
            help="This is the public key for this bunker."
          >
            <UInput
              :model-value="config ? config['pubkey'] : ''"
              disabled
              icon="i-heroicons-key"
            />
          </UFormField>

          <div class="border-t border-neutral-200 dark:border-neutral-800 pt-6">
            <h4 class="font-medium mb-4">
              Update NSEC Source
            </h4>

            <div class="space-y-4">
              <UFormField
                label="BUNKER_NSEC"
                help="Enter the hex or bech32 nsec."
              >
                <UInput
                  v-model="state.nsec"
                  type="password"
                  placeholder="nsec1..."
                />
              </UFormField>

              <div class="flex items-center gap-4">
                <div class="h-px bg-neutral-200 dark:bg-neutral-800 flex-grow" />
                <span class="text-xs text-neutral-500 font-medium">OR</span>
                <div class="h-px bg-neutral-200 dark:bg-neutral-800 flex-grow" />
              </div>

              <UFormField
                label="BUNKER_NSEC_FILE"
                help="Path to file containing nsec."
              >
                <UInput
                  v-model="state.nsecFile"
                  placeholder="/etc/bunker/nsec"
                />
              </UFormField>
            </div>
          </div>
        </div>

        <template #footer>
          <div class="flex justify-end">
            <UButton
              :loading="loading"
              color="primary"
              @click="saveConfig"
            >
              Save Configuration
            </UButton>
          </div>
        </template>
      </UCard>
    </template>
  </UDashboardPanel>
</template>
