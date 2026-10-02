// The NIP-05 invariants from Porwr's brief and its stale-result bug, mirroring
// porwr/source/tests/unit/components/ProfileEditor.test.ts (#30).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import IdentityVerification from '~/components/profile/IdentityVerification.vue'

const pubkey = 'ab'.repeat(32)
const fetchMock = vi.fn<typeof fetch>()
const names = (map: Record<string, string>) => Response.json({ names: map })

async function mount(identifier: string) {
  const component = await mountSuspended(IdentityVerification, {
    props: {
      'pubkey': pubkey,
      'modelValue': identifier,
      'onUpdate:modelValue': (value: string) => component.setProps({ modelValue: value })
    }
  })
  return component
}

const status = (component: Awaited<ReturnType<typeof mount>>) => component.find('[data-testid="nip05-status"]')
const verifyButton = (component: Awaited<ReturnType<typeof mount>>) => component.find('[data-testid="verify-nip05"]')

describe('Identity Verification', () => {
  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })
  afterEach(() => vi.unstubAllGlobals())

  it('never verifies on input', async () => {
    const component = await mount('')
    await component.find('input').setValue('alice@example.com')
    await component.find('input').setValue('alice@example.org')
    await flushPromises()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(status(component).attributes('data-tone')).toBe('neutral')
  })

  it('keeps Verify disabled for a malformed identifier, and says why', async () => {
    const component = await mount('not-an-identifier')
    expect(verifyButton(component).attributes('disabled')).toBeDefined()
    expect(status(component).text()).toContain('name@example.com')
    await component.find('input').setValue('alice@example.com')
    expect(verifyButton(component).attributes('disabled')).toBeUndefined()
  })

  it('shows verified, naming the domain', async () => {
    fetchMock.mockResolvedValue(names({ alice: pubkey }))
    const component = await mount('alice@example.com')
    await verifyButton(component).trigger('click')
    await flushPromises()
    expect(status(component).attributes('data-tone')).toBe('verified')
    expect(status(component).text()).toContain('Verified')
    expect(status(component).text()).toContain('example.com returned this account\'s public key.')
  })

  it('shows a failure, and the mismatch copy for another key', async () => {
    fetchMock.mockResolvedValue(names({ alice: 'cd'.repeat(32) }))
    const component = await mount('alice@example.com')
    await verifyButton(component).trigger('click')
    await flushPromises()
    expect(status(component).attributes('data-tone')).toBe('failed')
    expect(status(component).text()).toContain('Unable to verify this identifier.')
    expect(status(component).text()).toContain('This identifier resolves to a different public key.')

    fetchMock.mockResolvedValue(names({}))
    await component.find('input').setValue('bob@example.com')
    await verifyButton(component).trigger('click')
    await flushPromises()
    expect(status(component).text()).toContain('The domain does not list this name.')
  })

  it('resets the status when the value changes', async () => {
    fetchMock.mockResolvedValue(names({ alice: pubkey }))
    const component = await mount('alice@example.com')
    await verifyButton(component).trigger('click')
    await flushPromises()
    expect(status(component).attributes('data-tone')).toBe('verified')

    await component.find('input').setValue('alice@example.org')
    expect(status(component).attributes('data-tone')).toBe('neutral')
  })

  it('drops a result that arrives after the value changed (Porwr\'s stale-result bug)', async () => {
    let answer: (response: Response) => void = () => {}
    fetchMock.mockImplementation(() => new Promise(resolve => (answer = resolve)))
    const component = await mount('alice@example.com')
    await verifyButton(component).trigger('click')
    expect(status(component).attributes('data-tone')).toBe('verifying')

    await component.find('input').setValue('bob@example.com')
    answer(names({ alice: pubkey }))
    await flushPromises()

    expect(status(component).attributes('data-tone')).toBe('neutral')
    expect(status(component).text()).not.toContain('Verified')
  })
})
