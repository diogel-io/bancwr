// An identity's profile, follows and relays (#150): the member's own pages, at
// /identities/:pubkey/*, editing the identity and signed by the bunker as it, with a banner naming
// it. The member's own signer is never asked, and an unknown key edits nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockNuxtImport, mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { readBody } from 'h3'
import { finalizeEvent, generateSecretKey, getPublicKey, verifyEvent } from 'nostr-tools/pure'
import { UApp } from '#components'
import { clearNuxtData } from '#imports'
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
    const name = component.findAll('label').find(l => l.text().trim() === 'Name')!
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
    expect(relays.filters.some(f => f.kinds?.includes(3) && f.authors?.includes(SALES))).toBe(true)
    expect(relays.filters.some(f => f.authors?.includes(me))).toBe(false)
  })

  it('says there is no such identity, and reads and edits nothing', async () => {
    const component = await mount(Profile, `/identities/${'9'.repeat(64)}/profile`)
    await vi.waitFor(() => expect(component.find('[data-testid="editing-identity-missing"]').exists()).toBe(true))
    expect(component.find('[data-testid="profile-save"]').exists()).toBe(false)

    const notAKey = await mount(Profile, '/identities/not-a-key/profile')
    expect(notAKey.find('[data-testid="editing-identity-missing"]').exists()).toBe(true)
    expect(relays.filters.some(f => f.authors?.includes(me))).toBe(false)
  })
})
