// An identity's profile, follows and relays (#150): the member's own pages, at
// /identities/:pubkey/*, editing the identity and signed by the bunker as it, with a banner naming
// it. The member's own signer is never asked, and an unknown key edits nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockNuxtImport, mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { readBody } from 'h3'
import { finalizeEvent, generateSecretKey, getPublicKey, verifyEvent } from 'nostr-tools/pure'
import { NuxtPage, UApp } from '#components'
import { clearNuxtData, useRouter } from '#imports'
import { FakeRelays, nostrEvent, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import Profile from '~/pages/profile.vue'
import Follows from '~/pages/follows.vue'
import type { BunkerIdentity } from '#shared/types/bunker'

const secret = generateSecretKey()
const SALES = getPublicKey(secret)
const me = 'a'.repeat(64)

const { holder } = vi.hoisted(() => ({
  holder: { io: undefined as unknown, signs: [] as unknown[], ownSigner: vi.fn() }
}))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))
mockNuxtImport('useUserSigner', () => () => ({ signer: holder.ownSigner }))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint('/api/bunker/identities', {
  method: 'GET',
  handler: (): BunkerIdentity[] => [{ pubkey: SALES, npub: 'npub1sales', label: 'Sales', is_primary: false, available: true, members: [] }]
})
// The bunker signing as the identity.
registerEndpoint(`/api/bunker/identities/${SALES}/sign`, {
  method: 'POST',
  handler: async (event) => {
    const template = await readBody(event)
    holder.signs.push({ ...template })
    return finalizeEvent(template, secret)
  }
})

/** The profile form's Name label, once the identity is known and its profile read. */
async function nameLabel(component: Awaited<ReturnType<typeof mount>>) {
  return vi.waitFor(() => {
    const label = component.findAll('label').find(l => l.text().trim() === 'Name')
    if (!label) throw new Error('no Name field yet')
    return label
  })
}

let mounted: { unmount: () => void }[] = []
async function mount(page: unknown, route: string) {
  const InApp = defineComponent({ render: () => h(UApp, () => h(page as never)) })
  const component = await mountSuspended(InApp, { attachTo: document.body, route })
  mounted.push(component)
  await flushPromises()
  return component
}

describe('Editing an identity (#150)', () => {
  let relays: FakeRelays

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    signInAs('administrator')
    relays = new FakeRelays()
    holder.io = relays
    holder.signs = []
    holder.ownSigner.mockReset()
  })

  afterEach(() => {
    for (const component of mounted) component.unmount()
    mounted = []
  })

  it('edits the identity\'s profile, names it, and has the bunker sign as it', async () => {
    relays.events.push(profileEvent(me, { name: 'me' }, 100), profileEvent(SALES, { name: 'Sales', about: 'kept' }, 100))
    const component = await mount(Profile, `/identities/${SALES}/profile`)

    expect(component.find('[data-testid="editing-identity"]').text()).toContain('Editing Sales')
    expect(component.find('[data-testid="signed-in-as"]').exists()).toBe(false)
    const name = await nameLabel(component)
    const input = component.find(`#${name.attributes('for')}`)
    expect((input.element as HTMLInputElement).value).toBe('Sales')

    await input.setValue('Sales team')
    await component.find('[data-testid="profile-save"]').trigger('click')
    await vi.waitFor(() => expect(relays.published).toHaveLength(1))

    const event = relays.published[0]!
    expect(event.pubkey).toBe(SALES)
    expect(verifyEvent(JSON.parse(JSON.stringify(event)))).toBe(true)
    expect(JSON.parse(event.content)).toEqual({ name: 'Sales team', about: 'kept' })
    expect(holder.signs).toHaveLength(1)
    expect(Object.keys(holder.signs[0] as object).sort()).toEqual(['content', 'created_at', 'kind', 'tags'])
    expect(holder.ownSigner).not.toHaveBeenCalled()
  })

  it('edits the identity\'s follows the same way', async () => {
    relays.events.push(nostrEvent(SALES, 3, [], 100))
    const component = await mount(Follows, `/identities/${SALES}/follows`)
    expect(component.find('[data-testid="editing-identity"]').text()).toContain('Editing Sales')
    await vi.waitFor(() => expect(relays.filters.some(f => f.kinds?.includes(3) && f.authors?.includes(SALES))).toBe(true))
    expect(relays.filters.some(f => f.authors?.includes(me))).toBe(false)
  })

  it('asks before going from an identity\'s profile to the member\'s own with unsaved changes', async () => {
    relays.events.push(profileEvent(me, { name: 'me' }, 100), profileEvent(SALES, { name: 'Sales' }, 100))
    // Through the router view, as in the app: in-component route guards only run there.
    const component = await mount(NuxtPage, `/identities/${SALES}/profile`)
    const name = await nameLabel(component)
    const input = component.find(`#${name.attributes('for')}`)
    await input.setValue('Sales team')

    const confirm = vi.fn(() => false)
    vi.stubGlobal('confirm', confirm)
    const router = useRouter()
    await router.push('/profile')
    expect(confirm).toHaveBeenCalledWith('Leave without saving your profile changes?')
    expect(router.currentRoute.value.path).toBe(`/identities/${SALES}/profile`)
    expect((input.element as HTMLInputElement).value).toBe('Sales team')

    confirm.mockReturnValue(true)
    await router.push('/profile')
    expect(router.currentRoute.value.path).toBe('/profile')
    vi.unstubAllGlobals()
  })

  it('asks before going from one identity\'s profile to another\'s with unsaved changes', async () => {
    relays.events.push(profileEvent(SALES, { name: 'Sales' }, 100))
    const component = await mount(NuxtPage, `/identities/${SALES}/profile`)
    const name = await nameLabel(component)
    await component.find(`#${name.attributes('for')}`).setValue('Sales team')

    const confirm = vi.fn(() => false)
    vi.stubGlobal('confirm', confirm)
    const router = useRouter()
    await router.push(`/identities/${'9'.repeat(64)}/profile`)
    expect(confirm).toHaveBeenCalledWith('Leave without saving your profile changes?')
    expect(router.currentRoute.value.path).toBe(`/identities/${SALES}/profile`)
    vi.unstubAllGlobals()
  })

  it('is a route of its own, not an alias of the member\'s page', async () => {
    const router = useRouter()
    for (const page of ['profile', 'follows', 'relays']) {
      const identity = router.resolve(`/identities/${SALES}/${page}`)
      expect(identity.name).not.toBe(router.resolve(`/${page}`).name)
      expect(identity.matched.at(-1)?.aliasOf).toBeUndefined()
      // A relative location keeps the identity's page.
      await router.push(identity)
      expect(router.resolve({ query: { tab: '1' } }).path).toBe(`/identities/${SALES}/${page}`)
    }
  })

  it('says there is no such identity, and reads and edits nothing', async () => {
    const component = await mount(Profile, `/identities/${'9'.repeat(64)}/profile`)
    await vi.waitFor(() => expect(component.find('[data-testid="editing-identity-missing"]').exists()).toBe(true))
    expect(component.find('[data-testid="profile-save"]').exists()).toBe(false)
    expect(relays.queries).toBe(0)
    expect(holder.signs).toHaveLength(0)

    const notAKey = await mount(Profile, '/identities/not-a-key/profile')
    expect(notAKey.find('[data-testid="editing-identity-missing"]').exists()).toBe(true)
    expect(relays.queries).toBe(0)
    expect(holder.signs).toHaveLength(0)
  })
})
