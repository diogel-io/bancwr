import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { useState } from '#imports'
import { signInAs } from '../helpers/session'
import SignedInAs from '~/components/SignedInAs.vue'

const { found } = vi.hoisted(() => ({ found: { value: undefined as unknown } }))
vi.mock('~/utils/key-profile', () => ({
  lookupKeyProfile: async () => found.value,
  lookupRelays: () => [],
  queryRelays: async () => []
}))

describe('SignedInAs (#70)', () => {
  // The lookup is shared, once per key (#72); each test starts without one.
  beforeEach(() => {
    useState('signed-in-profile').value = { pubkey: '', status: 'idle' }
  })

  it('names the signed-in key, with its npub', async () => {
    found.value = { name: 'Alice', createdAt: 1 }
    signInAs('signer')
    const component = await mountSuspended(SignedInAs)
    await flushPromises()
    expect(component.find('[data-testid="signed-in-as"]').text()).toContain('Signed in as Alice')
    expect(component.text()).toContain('npub1a')
  })

  it('falls back to a short npub without a profile', async () => {
    found.value = undefined
    signInAs('signer')
    const component = await mountSuspended(SignedInAs)
    await flushPromises()
    expect(component.find('[data-testid="signed-in-as"]').text()).toContain('Signed in as npub1a')
  })
})
