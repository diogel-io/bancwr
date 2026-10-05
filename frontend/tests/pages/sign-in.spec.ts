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
// No relays from tests: the confirmation's name lookup (#70) finds a name, or none.
const { keyProfile } = vi.hoisted(() => ({ keyProfile: { value: undefined as unknown } }))
vi.mock('~/utils/key-profile', () => ({
  lookupKeyProfile: async () => keyProfile.value,
  lookupRelays: () => [],
  queryRelays: async () => []
}))
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

/** Sign in with extension, then continue as the key it chose (#70). */
async function signInWithExtension(component: Awaited<ReturnType<typeof mount>>) {
  await button(component, 'Sign in with extension')!.trigger('click')
  await flushPromises()
  await component.find('[data-testid="confirm-continue"]').trigger('click')
  await flushPromises()
}

describe('Sign-in page', () => {
  beforeEach(() => {
    for (const fn of [signInWithNostr, refresh, navigateTo, connect, cancel, signer.getPublicKey, signer.signEvent]) fn.mockReset()
    signer.getPublicKey.mockResolvedValue(extensionPubkey)
    keyProfile.value = undefined
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
    await signInWithExtension(component)
    // A wrapper that checks the key it signs with is the one confirmed (#70).
    expect(signInWithNostr).toHaveBeenCalledWith(expect.objectContaining({ signEvent: expect.any(Function) }), window.location.origin)
    expect(refresh).toHaveBeenCalled()
    expect(navigateTo).toHaveBeenCalledWith('/')
  })

  it('sends an unregistered key to the no-access page', async () => {
    signInWithNostr.mockResolvedValue({ status: 403, error: 'not_registered', npub: 'npub1x' })
    const component = await mount(true)
    await signInWithExtension(component)
    expect(navigateTo).toHaveBeenCalledWith('/no-access')
  })

  it('says so when the extension declines', async () => {
    signInWithNostr.mockRejectedValue(new Error('User rejected'))
    const component = await mount(true)
    await signInWithExtension(component)
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
      ['bunker_key', 403, 'bunker\'s own key'],
      ['bunker_key', 403, 'the first administrator is set with BANCWR_ADMIN_PUBKEY']
    ]
    const component = await mount(true)
    for (const [reason, status, text] of reasons) {
      signInWithNostr.mockResolvedValueOnce({ status, error: status === 401 ? 'not_authenticated' : 'forbidden', reason })
      await signInWithExtension(component)
      expect(component.find('[data-testid="sign-in-error"]').text(), reason).toContain(text)
    }
    expect(navigateTo).not.toHaveBeenCalled()
  })

  it('asks the extension for its key, shows it, and signs nothing until continued (#70)', async () => {
    signInWithNostr.mockImplementation(never)
    keyProfile.value = { name: 'Alice', createdAt: 1 }
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()

    expect(component.find('[data-testid="confirm-npub"]').text()).toBe(npubEncode(extensionPubkey))
    expect(component.find('[data-testid="confirm-name"]').text()).toBe('Alice')
    expect(component.find('[data-testid="confirm-continue"]').text()).toBe('Continue as Alice')
    expect(signInWithNostr).not.toHaveBeenCalled()
    expect(signer.signEvent).not.toHaveBeenCalled()

    await component.find('[data-testid="confirm-continue"]').trigger('click')
    await flushPromises()
    expect(signInWithNostr).toHaveBeenCalled()
  })

  it('loads the key\'s picture without a referrer, the policy set before src (#72)', async () => {
    keyProfile.value = { name: 'Alice', picture: 'https://img.example/alice.png', createdAt: 1 }
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()

    const img = component.find('[data-testid="confirm-key"] img')
    expect(img.attributes('src')).toBe('https://img.example/alice.png')
    const order = (img.element as HTMLImageElement).getAttributeNames()
    expect(order[0]).toBe('referrerpolicy')
    expect(img.attributes('referrerpolicy')).toBe('no-referrer')
  })

  it('shows a short npub when the key has no profile', async () => {
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()
    const npub = npubEncode(extensionPubkey)
    expect(component.find('[data-testid="confirm-name"]').text()).toBe(`${npub.slice(0, 12)}…${npub.slice(-6)}`)
  })

  it('signs nothing on Use another key, and says how to switch keys in the extension (#70)', async () => {
    const component = await mount(true)
    await button(component, 'Sign in with extension')!.trigger('click')
    await flushPromises()
    await component.find('[data-testid="confirm-another"]').trigger('click')
    await flushPromises()

    expect(signInWithNostr).not.toHaveBeenCalled()
    expect(component.find('[data-testid="confirm-key"]').exists()).toBe(false)
    const guidance = component.find('[data-testid="another-key-guidance"]').text()
    expect(guidance).toContain('keep using the key a site first connected with')
    expect(guidance).toContain(window.location.host)
    // And the extension button is back, to try again.
    expect(button(component, 'Sign in with extension')).toBeDefined()
  })

  it('refuses a signer that signs with another key than the one confirmed (#70)', async () => {
    const other = 'cd'.repeat(32)
    signer.signEvent.mockResolvedValue({ kind: 27235, created_at: 1, tags: [], content: '', id: 'e'.repeat(64), pubkey: other, sig: 'f'.repeat(128) })
    // As the real one does: ask the signer to sign the login event.
    signInWithNostr.mockImplementation(async (wrapped: { signEvent: (t: unknown) => Promise<unknown> }) => {
      await wrapped.signEvent({ kind: 27235, created_at: 1, tags: [], content: '' })
      return { status: 200, pubkey: other, npub: 'npub1other', role: 'signer' }
    })
    const component = await mount(true)
    await signInWithExtension(component)

    expect(component.find('[data-testid="sign-in-error"]').text()).toContain('not the key you confirmed')
    expect(navigateTo).not.toHaveBeenCalled()
  })

  it('confirms a remote signer\'s key too, and Use another key disconnects it (#70)', async () => {
    connect.mockResolvedValue({ getPublicKey: async () => extensionPubkey, signEvent: vi.fn() })
    const component = await mount(false)
    await component.find('input').setValue(`bunker://${'cd'.repeat(32)}?relay=wss://relay.example`)
    await button(component, 'Connect and sign in')!.trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="confirm-npub"]').text()).toBe(npubEncode(extensionPubkey))
    expect(signInWithNostr).not.toHaveBeenCalled()

    await component.find('[data-testid="confirm-another"]').trigger('click')
    await flushPromises()
    expect(cancel).toHaveBeenCalled()
    expect(component.find('[data-testid="another-key-guidance"]').text()).toContain('connection string')
    expect((component.find('input').element as HTMLInputElement).value).toBe('')
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
