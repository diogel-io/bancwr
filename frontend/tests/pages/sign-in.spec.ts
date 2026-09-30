import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { ref } from 'vue'
import { npubEncode } from 'nostr-tools/nip19'
import SignIn from '~/pages/sign-in.vue'

const { signInWithNostr, refresh, navigateTo, connect, cancel, extension, remote } = vi.hoisted(() => ({
  signInWithNostr: vi.fn(),
  refresh: vi.fn(),
  navigateTo: vi.fn(),
  connect: vi.fn(),
  cancel: vi.fn(),
  extension: { available: undefined as unknown, signer: undefined as unknown },
  // The remote signer's progress, set per test so the page's waiting states can be driven.
  remote: { phase: undefined as unknown, approvalUrl: undefined as unknown }
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
  phase: remote.phase ?? ref('idle'),
  approvalUrl: remote.approvalUrl ?? ref(undefined),
  connect,
  cancel,
  settle: vi.fn()
}))

const extensionPubkey = 'ab'.repeat(32)
const signer = { getPublicKey: vi.fn(), signEvent: vi.fn() }

async function mount(present: boolean | undefined) {
  extension.available = ref(present)
  extension.signer = present ? signer : undefined
  remote.phase = ref('idle')
  remote.approvalUrl = ref(undefined)
  return mountSuspended(SignIn)
}

const never = () => new Promise(() => {})

const button = (component: Awaited<ReturnType<typeof mount>>, text: string) =>
  component.findAll('button').find(b => b.text().includes(text))

describe('Sign-in page', () => {
  beforeEach(() => {
    for (const fn of [signInWithNostr, refresh, navigateTo, connect, cancel, signer.getPublicKey]) fn.mockReset()
    signer.getPublicKey.mockResolvedValue(extensionPubkey)
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

  it('explains every refusal in plain language', async () => {
    const reasons: [string, number, string][] = [
      ['wrong_url', 401, 'does not match the address'],
      ['stale_event', 401, 'arrived too late'],
      ['expired_challenge', 401, 'request expired'],
      ['unknown_challenge', 401, 'request expired'],
      ['bad_signature', 401, 'invalid signature'],
      ['wrong_kind', 401, 'different kind of event'],
      ['bunker_key', 403, 'bunker\'s own key']
    ]
    const component = await mount(true)
    for (const [reason, status, text] of reasons) {
      signInWithNostr.mockResolvedValueOnce({ status, error: status === 401 ? 'not_authenticated' : 'forbidden', reason })
      await button(component, 'Sign in with extension')!.trigger('click')
      await flushPromises()
      expect(component.find('[data-testid="sign-in-error"]').text(), reason).toContain(text)
    }
    expect(navigateTo).not.toHaveBeenCalled()
  })

  it('asks the extension for its key first, and shows it while waiting for the signature', async () => {
    signInWithNostr.mockImplementation(never)
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()

    expect(signer.getPublicKey.mock.invocationCallOrder[0]).toBeLessThan(signInWithNostr.mock.invocationCallOrder[0]!)
    expect(component.find('[data-testid="extension-npub"]').text()).toBe(`Signing in as ${npubEncode(extensionPubkey)}`)
  })

  it('treats a refusal to share the key like a declined signature, and signs nothing', async () => {
    signer.getPublicKey.mockRejectedValue(new Error('User rejected'))
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()

    expect(component.find('[data-testid="sign-in-error"]').text()).toContain('did not sign the request')
    expect(signInWithNostr).not.toHaveBeenCalled()
    expect(component.find('[data-testid="extension-npub"]').exists()).toBe(false)
  })

  it('shows the remote signer\'s progress, its approval link, and a working Cancel', async () => {
    connect.mockImplementation(never)
    const component = await mount(false)
    await component.find('input').setValue(`bunker://${'cd'.repeat(32)}?relay=wss://relay.example`)
    await button(component, 'Connect and sign in')!.trigger('click')
    await flushPromises()

    // Pending, never a bare spinner: what it is waiting for, and a way out.
    ;(remote.phase as { value: string }).value = 'connecting'
    await flushPromises()
    expect(component.find('[role="status"]').text()).toContain('Connecting to your signer')
    expect(button(component, 'Connect and sign in')).toBeUndefined()

    ;(remote.phase as { value: string }).value = 'awaiting-approval'
    ;(remote.approvalUrl as { value: string }).value = 'https://signer.example/approve/1'
    await flushPromises()
    expect(component.find('[role="status"]').text()).toContain('asking you to approve')
    const approve = component.findAll('a').find(a => a.text().includes('Approve in your signer'))!
    expect(approve.attributes('href')).toBe('https://signer.example/approve/1')
    expect(approve.attributes('target')).toBe('_blank')
    expect(approve.attributes('rel')).toContain('noopener')

    ;(remote.phase as { value: string }).value = 'signing'
    await flushPromises()
    expect(component.find('[role="status"]').text()).toContain('Waiting for your signer to sign')

    await button(component, 'Cancel')!.trigger('click')
    expect(cancel).toHaveBeenCalled()
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
