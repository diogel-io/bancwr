import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import { FakeRelays, nostrEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import Relays from '~/pages/relays.vue'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'
import { useState } from '#imports'

const me = 'a'.repeat(64)
const EVERY_RELAY = [...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS]
const { holder } = vi.hoisted(() => ({ holder: { io: undefined as unknown, signer: undefined as unknown, nip46: true } }))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))
mockNuxtImport('useUserSigner', () => () => ({ signer: async () => holder.signer }))

registerEndpoint('/api/bunker/status', () => ({
  status: 'healthy', pubkey: 'npub1test', version: '0.1.0',
  checks: [holder.nip46
    ? { name: 'relays', status: 'pass', detail: 'All 1 relays connected.', relays: [{ url: 'wss://bunker-relay.example', connected: true }] }
    : { name: 'relays', status: 'disabled', detail: 'NIP-46 is turned off.' }]
}))

const signer: NostrSigner = {
  signEvent: async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent
}
const InApp = defineComponent({ render: () => h(UApp, () => h(Relays)) })

let relays: FakeRelays

async function mount() {
  const component = await mountSuspended(InApp, { attachTo: document.body })
  await flushPromises()
  return component
}

interface Mounted {
  find: (selector: string) => { setValue: (value: string) => Promise<void>, trigger: (event: string) => Promise<void>, text: () => string, exists: () => boolean }
}

async function addRelay(component: Mounted, value: string) {
  await component.find('[data-testid="add-relay"] input').setValue(value)
  await component.find('[data-testid="add-relay"]').trigger('submit')
}

describe('Relays page', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    signInAs('user')
    relays = new FakeRelays()
    holder.io = relays
    holder.signer = signer
    holder.nip46 = true
    useState('bunker-health').value = { state: 'unknown', checks: [] }
  })

  it('lists relays with their markers, and saves a marker change, a removal and an add', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://b.example', 'read']], 100))
    const component = await mount()
    const row = (url: string) => component.find(`[data-relay="${url}"]`)
    expect(row('wss://a.example').text()).toContain('a.example')

    // Make a.example write-only, remove b.example, add c.example.
    await row('wss://a.example').findAll('[role="checkbox"]')[0]!.trigger('click')
    await row('wss://b.example').findAll('button').find(b => b.text() === 'Remove')!.trigger('click')
    await addRelay(component, 'wss://C.example/')
    expect(row('wss://c.example').attributes('data-pending')).toBe('add')

    await component.find('[data-testid="relays-save"]').trigger('click')
    await flushPromises()
    expect(relays.published[0]!.tags).toEqual([['r', 'wss://a.example', 'write'], ['r', 'wss://c.example']])
    expect(component.find('[data-testid="relays-saved"]').exists()).toBe(true)
  })

  it('rejects an invalid or duplicate relay before staging it', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example']], 100))
    const component = await mount()
    await addRelay(component, 'https://a.example')
    expect(component.find('[data-testid="add-relay"]').text()).toContain('Relay addresses start with wss://')
    await addRelay(component, 'wss://A.example/')
    expect(component.find('[data-testid="add-relay"]').text()).toContain('already in your list')
    expect(component.find('[data-testid="relays-save"]').attributes('disabled')).toBeDefined()
  })

  it('states the default when there is no list, and starts one from the defaults once confirmed', async () => {
    const component = await mount()
    const statement = component.find('[data-testid="relays-default"]').text()
    expect(statement).toContain('You have no relay list')
    expect(statement).toContain('relay.damus.io')
    expect(component.find('[data-testid="relays-not-found"]').exists()).toBe(true)

    await component.find('[data-testid="relays-start-defaults"]').trigger('click')
    const save = component.find('[data-testid="relays-save"]')
    expect(save.text()).toBe('Start relay list')
    expect(save.attributes('disabled')).toBeDefined()
    // The confirmation is the last checkbox: the staged rows have Read and Write ones.
    await component.findAll('[role="checkbox"]').at(-1)!.trigger('click')
    await save.trigger('click')
    await flushPromises()
    expect(relays.published[0]!.tags).toEqual(DEFAULT_PROFILE_RELAYS.map(url => ['r', url.replace(/\/$/, '')]))
  })

  it('shows an empty list, advice on size, and a warning with no write relays', async () => {
    relays.events.push(nostrEvent(me, 10002, [], 100))
    const empty = await mount()
    expect(empty.find('[data-testid="relays-default"]').text()).toContain('Your relay list is empty')
    empty.unmount()

    document.body.innerHTML = ''
    relays.events = [nostrEvent(me, 10002, Array.from({ length: 5 }, (_, i) => ['r', `wss://r${i}.example`, 'read']), 200)]
    const component = await mount()
    expect(component.find('[data-testid="relays-too-many"]').text()).toContain('5 read and 0 write')
    expect(component.find('[data-testid="relays-no-write"]').exists()).toBe(true)
  })

  it('says why a save was refused, and publishes nothing', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example']], 100))
    const component = await mount()
    await addRelay(component, 'wss://b.example')
    relays.down = new Set([...EVERY_RELAY, 'wss://a.example/'])
    await component.find('[data-testid="relays-save"]').trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="relays-save-error"]').text()).toContain('could drop relays')
    expect(relays.published).toEqual([])
  })

  it('shows the bunker\'s own relays as context, and nothing when NIP-46 is off', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example']], 100))
    const component = await mount()
    await vi.waitFor(() => expect(component.find('[data-testid="bunker-relays"]').text()).toContain('bunker-relay.example'))
    expect(component.find('[data-testid="bunker-relays"]').text()).toContain('separate from your list')
    component.unmount()

    document.body.innerHTML = ''
    holder.nip46 = false
    useState('bunker-health').value = { state: 'unknown', checks: [] }
    const off = await mount()
    await flushPromises()
    expect(off.find('[data-testid="bunker-relays"]').exists()).toBe(false)
  })

  it('flags an unencrypted remote relay, but not a local one', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'ws://relay.example.com'], ['r', 'ws://localhost:8080', 'read']], 100))
    const component = await mount()
    expect(component.find('[data-relay="ws://relay.example.com"]').text()).toContain('Unencrypted')
    expect(component.find('[data-relay="ws://localhost:8080"]').text()).not.toContain('Unencrypted')
  })
})
