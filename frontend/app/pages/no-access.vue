<script setup lang="ts">
// A key that signed in correctly but is not registered in this bunker's vault (#11).
definePageMeta({ layout: 'auth' })

const auth = useAuth()
const npub = computed(() => auth.state.value.status === 'not-registered' ? auth.state.value.npub : '')
const copied = ref(false)

async function copy() {
  await navigator.clipboard.writeText(npub.value)
  copied.value = true
}
</script>

<template>
  <div class="space-y-4">
    <h1 class="text-lg font-semibold">
      This key is not registered with this bunker
    </h1>
    <p class="text-sm text-muted">
      You signed in, but only keys registered in the vault can use Bancwr. Contact the vault
      administrator and ask them to add this key:
    </p>
    <div class="flex items-center gap-2">
      <code
        data-testid="presented-npub"
        class="text-xs break-all flex-1"
      >{{ npub }}</code>
      <UButton
        :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
        color="neutral"
        variant="ghost"
        :aria-label="copied ? 'Copied' : 'Copy npub'"
        @click="copy"
      />
    </div>
    <UButton
      block
      color="neutral"
      variant="outline"
      @click="auth.signOut()"
    >
      Sign out and try another key
    </UButton>
  </div>
</template>
