import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { ref } from 'vue'
import SignerConnect from '~/components/SignerConnect.vue'

const { extension, connect, cancel } = vi.hoisted(() => ({
  extension: { getPublicKey: vi.fn(), signEvent: vi.fn() },
  connect: vi.fn(),
  cancel: vi.fn()
}))
mockNuxtImport('useNip07', () => () => ({ available: ref(true), signer: () => extension }))
mockNuxtImport('useNip46', () => () => ({ phase: ref('idle'), approvalUrl: ref(undefined), connect, cancel, settle: vi.fn() }))

const mine = 'ab'.repeat(32)
const theirs = 'cd'.repeat(32)

describe('SignerConnect, reconnecting the member\'s signer (#30)', () => {
  beforeEach(() => {
    for (const fn of [extension.getPublicKey, connect, cancel]) fn.mockReset()
  })

  it('hands over an extension holding the signed-in key', async () => {
    extension.getPublicKey.mockResolvedValue(mine)
    const use = vi.fn(async () => undefined)
    const component = await mountSuspended(SignerConnect, { props: { use, expectedPubkey: mine } })
    await component.findAll('button').find(b => b.text() === 'Use extension')!.trigger('click')
    await flushPromises()
    expect(use).toHaveBeenCalledWith(extension, 'nip07')
  })

  it('refuses an extension holding another key, before anything is signed', async () => {
    extension.getPublicKey.mockResolvedValue(theirs)
    const use = vi.fn(async () => undefined)
    const component = await mountSuspended(SignerConnect, { props: { use, expectedPubkey: mine } })
    await component.findAll('button').find(b => b.text() === 'Use extension')!.trigger('click')
    await flushPromises()
    expect(use).not.toHaveBeenCalled()
    expect(component.find('[data-testid="signer-error"]').text()).toContain('not the key you are signed in with')
  })

  it('refuses and disconnects a remote signer holding another key', async () => {
    connect.mockResolvedValue({ getPublicKey: async () => theirs, signEvent: vi.fn() })
    const use = vi.fn(async () => undefined)
    const component = await mountSuspended(SignerConnect, { props: { use, expectedPubkey: mine } })
    await component.find('input').setValue(`bunker://${'ef'.repeat(32)}?relay=wss://relay.example`)
    await component.findAll('button').find(b => b.text() === 'Connect')!.trigger('click')
    await flushPromises()
    expect(use).not.toHaveBeenCalled()
    expect(cancel).toHaveBeenCalled()
    expect(component.find('[data-testid="signer-error"]').text()).toContain('not the key you are signed in with')
  })
})
