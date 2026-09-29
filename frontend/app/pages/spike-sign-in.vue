<script setup lang="ts">
// SPIKE (diogel-io/bancwr#23): exercises sign-in with a NIP-07 signer in a real browser, so the
// session cookie's Secure / SameSite=Strict / HttpOnly behaviour is tested, not assumed. #11
// replaces this with the real sign-in screen.
import { ref } from 'vue'
import { signInWithNostr, type NostrSigner } from '~/utils/nostr-sign-in'

const output = ref('')

async function signIn() {
  const nostr = (window as unknown as { nostr?: NostrSigner }).nostr
  if (!nostr) {
    output.value = 'no NIP-07 signer'
    return
  }
  output.value = JSON.stringify(await signInWithNostr(nostr, window.location.origin))
}

async function call(path: string, method: 'GET' | 'POST' = 'GET') {
  const response = await fetch(path, { method, credentials: 'same-origin' })
  output.value = JSON.stringify({ status: response.status, body: await response.json().catch(() => null) })
}
</script>

<template>
  <div class="p-8 space-y-4">
    <div class="flex gap-2">
      <UButton @click="signIn">
        Sign in with extension
      </UButton>
      <UButton @click="call('/api/bunker/whoami')">
        Who am I (via proxy)
      </UButton>
      <UButton @click="call('/api/auth/logout', 'POST')">
        Sign out
      </UButton>
    </div>
    <pre data-testid="output">{{ output }}</pre>
  </div>
</template>
