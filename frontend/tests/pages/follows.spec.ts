import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockNuxtImport, mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { npubEncode } from 'nostr-tools/nip19'
import { UApp } from '#components'
import { FakeRelays, nostrEvent, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import Follows from '~/pages/follows.vue'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const me = 'a'.repeat(64)
const key = (n: number) => n.toString(16).padStart(64, '0')
const EVERY_RELAY = [...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS]
const { holder } = vi.hoisted(() => ({ holder: { io: undefined as unknown, signer: undefined as unknown } }))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))
mockNuxtImport('useUserSigner', () => () => ({ signer: async () => holder.signer }))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))

const signer: NostrSigner = {
  signEvent: async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent
}
const InApp = defineComponent({ render: () => h(UApp, () => h(Follows)) })

let relays: FakeRelays
/** What the helpers need of a mounted component. */
interface Mounted {
  find: (selector: string) => { setValue: (value: string) => Promise<void>, trigger: (event: string) => Promise<void> }
  findAll: (selector: 'button') => { text: () => string, trigger: (event: string) => Promise<void> }[]
}

async function mount() {
  const component = await mountSuspended(InApp, { attachTo: document.body })
  await flushPromises()
  return component
}

async function find(component: Mounted, value: string) {
  await component.find('[data-testid="add-follow"] input').setValue(value)
  await component.find('[data-testid="add-follow"] form').trigger('submit')
  await flushPromises()
}

const button = (component: Mounted, text: string) => component.findAll('button').find(b => b.text() === text)

describe('Follows page', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    signInAs('signer')
    relays = new FakeRelays()
    holder.io = relays
    holder.signer = signer
  })
  afterEach(() => vi.unstubAllGlobals())

  it('lists follows with their names, and saves an add and a remove', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)], ['p', key(2)]], 100))
    relays.events.push(profileEvent(key(1), { display_name: 'Alice' }, 1), profileEvent(key(3), { name: 'carol' }, 1))
    const component = await mount()
    await vi.waitFor(() => expect(component.find('[data-testid="follows-list"]').text()).toContain('Alice'))
    expect(component.find('[data-testid="follows-count"]').text()).toBe('2')

    await component.find(`[data-pubkey="${key(2)}"] button`).trigger('click')
    await find(component, npubEncode(key(3)))
    expect(component.find('[data-testid="add-follow-preview"]').text()).toContain('carol')
    await button(component, 'Add')!.trigger('click')
    expect(component.find(`[data-pubkey="${key(3)}"]`).attributes('data-pending')).toBe('add')
    expect(component.find(`[data-pubkey="${key(2)}"]`).attributes('data-pending')).toBe('remove')
    expect(component.find('[data-testid="follows-pending"]').text()).toBe('1 to follow, 1 to unfollow')

    await component.find('[data-testid="follows-save"]').trigger('click')
    await flushPromises()
    expect(relays.published[0]!.tags).toEqual([['p', key(1)], ['p', key(3)]])
    expect(component.find('[data-testid="follows-saved"]').exists()).toBe(true)
  })

  it('rejects malformed input, a key already followed, and resolves a NIP-05 identifier', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)]], 100))
    const component = await mount()

    await find(component, 'not a key')
    expect(component.find('[data-testid="add-follow"]').text()).toContain('Enter an npub')
    await find(component, key(1).toUpperCase())
    expect(component.find('[data-testid="add-follow"]').text()).toContain('You already follow this key.')

    vi.stubGlobal('fetch', vi.fn(async () => Response.json({ names: { bob: key(5) } })))
    await find(component, 'bob@example.com')
    expect(component.find('[data-testid="add-follow-preview"]').text()).toContain(npubEncode(key(5)))
    expect(relays.published).toEqual([])
  })

  it('shows an empty list, and a key with no list as not found, starting one only once confirmed', async () => {
    relays.events.push(nostrEvent(me, 3, [], 100))
    const empty = await mount()
    expect(empty.find('[data-testid="follows-empty"]').exists()).toBe(true)
    empty.unmount()

    relays = new FakeRelays()
    holder.io = relays
    document.body.innerHTML = ''
    const component = await mount()
    expect(component.find('[data-testid="follows-not-found"]').text()).toContain('No follow list found')
    await find(component, key(7))
    await button(component, 'Add')!.trigger('click')
    const save = component.find('[data-testid="follows-save"]')
    expect(save.text()).toBe('Start follow list')
    expect(save.attributes('disabled')).toBeDefined()
    await component.find('[role="checkbox"]').trigger('click')
    await save.trigger('click')
    await flushPromises()
    expect(relays.published[0]!.tags).toEqual([['p', key(7)]])
  })

  it('says why a save was refused, and publishes nothing', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)]], 100))
    const component = await mount()
    await component.find(`[data-pubkey="${key(1)}"] button`).trigger('click')
    relays.down = new Set(EVERY_RELAY)
    await component.find('[data-testid="follows-save"]').trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="follows-save-error"]').text()).toContain('could drop follows')
    expect(relays.published).toEqual([])
  })

  it('will not edit when no relay can be read', async () => {
    relays.down = new Set(EVERY_RELAY)
    const component = await mount()
    expect(component.find('[data-testid="follows-load-failed"]').exists()).toBe(true)
  })

  it('filters and pages a long list', async () => {
    relays.events.push(nostrEvent(me, 3, Array.from({ length: 120 }, (_, i) => ['p', key(i + 1)]), 100))
    relays.events.push(profileEvent(key(77), { name: 'zelda' }, 1))
    const component = await mount()
    expect(component.findAll('[data-testid="follows-list"] li')).toHaveLength(50)
    await vi.waitFor(() => expect(component.text()).not.toContain('Reading your follow list'))
    await component.find('input[aria-label="Filter follows"]').setValue('zelda')
    await vi.waitFor(() => expect(component.findAll('[data-testid="follows-list"] li')).toHaveLength(1))
  })
})
