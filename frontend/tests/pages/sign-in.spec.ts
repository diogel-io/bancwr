import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { ref } from 'vue'
import SignIn from '~/pages/sign-in.vue'

const { signInWithNostr, refresh, navigateTo, connect, cancel, extension } = vi.hoisted(() => ({
  signInWithNostr: vi.fn(),
  refresh: vi.fn(),
  navigateTo: vi.fn(),
  connect: vi.fn(),
  cancel: vi.fn(),
  extension: { available: undefined as unknown, signer: undefined as unknown }
}))

vi.mock('~/utils/nostr-sign-in', () => ({ signInWithNostr }))
mockNuxtImport('useAuth', () => () => ({ refresh }))
mockNuxtImport('navigateTo', () => navigateTo)
// Built when the page calls it, after mount() has set this test's values. The test app also
// renders /sign-in once at start-up, before any test has, hence the fallback. (Destructured
// hoisted values: mockNuxtImport factories do not see members of a hoisted object.)
mockNuxtImport('useNip07', () => () => ({
  available: extension.available ?? { value: undefined },
  signer: () => extension.signer
}))
mockNuxtImport('useNip46', () => () => ({
  phase: ref('idle'),
  approvalUrl: ref(undefined),
  connect,
  cancel,
  settle: vi.fn()
}))

const signer = { signEvent: vi.fn() }

async function mount(present: boolean | undefined) {
  extension.available = ref(present)
  extension.signer = present ? signer : undefined
  return mountSuspended(SignIn)
}

const button = (component: Awaited<ReturnType<typeof mount>>, text: string) =>
  component.findAll('button').find(b => b.text().includes(text))

describe('Sign-in page', () => {
  beforeEach(() => {
    for (const fn of [signInWithNostr, refresh, navigateTo, connect, cancel]) fn.mockReset()
  })

  it('explains a missing extension and keeps the remote signer available', async () => {
    const component = await mount(false)
    expect(component.text()).toContain('No Nostr signing extension was found')
    expect(button(component, 'Sign in with extension')).toBeUndefined()
    expect(component.text()).toContain('Remote signer (NIP-46)')
    expect(component.find('input').exists()).toBe(true)
  })

  it('signs in with the extension and goes to the dashboard', async () => {
    signInWithNostr.mockResolvedValue({ status: 200, pubkey: 'a', npub: 'npub1a', role: 'administrator' })
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()
    expect(signInWithNostr).toHaveBeenCalledWith(signer, window.location.origin)
    expect(refresh).toHaveBeenCalled()
    expect(navigateTo).toHaveBeenCalledWith('/')
  })

  it('sends an unregistered key to the no-access page', async () => {
    signInWithNostr.mockResolvedValue({ status: 403, error: 'not_registered', npub: 'npub1x' })
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()
    expect(navigateTo).toHaveBeenCalledWith('/no-access')
  })

  it('says so when the extension declines', async () => {
    signInWithNostr.mockRejectedValue(new Error('User rejected'))
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="sign-in-error"]').text()).toContain('did not sign the request')
    expect(navigateTo).not.toHaveBeenCalled()
  })

  it('explains each refusal in plain language', async () => {
    const component = await mount(true)
    for (const [reason, text] of [['wrong_url', 'does not match the address'], ['bunker_key', 'bunker\'s own key'], ['bad_signature', 'invalid signature']]) {
      signInWithNostr.mockResolvedValueOnce({ status: reason === 'bunker_key' ? 403 : 401, error: 'x', reason })
      await button(component, 'Sign in with extension')!.trigger('click')
      await flushPromises()
      expect(component.find('[data-testid="sign-in-error"]').text(), reason).toContain(text)
    }
  })

  it('shows a remote signer timeout, and the page stays usable', async () => {
    connect.mockRejectedValue(new Error('The signer did not answer. Check the relay in the connection string, and that the signer is online.'))
    const component = await mount(false)
    await component.find('input').setValue(`bunker://${'ab'.repeat(32)}?relay=wss://relay.example`)
    await button(component, 'Connect and sign in')!.trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="sign-in-error"]').text()).toContain('did not answer')
    expect(cancel).toHaveBeenCalled()
    expect(button(component, 'Connect and sign in')!.attributes('disabled')).toBeUndefined()
  })
})
