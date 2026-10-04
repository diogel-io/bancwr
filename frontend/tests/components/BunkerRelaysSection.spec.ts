// The bunker relays section of the Config page (#78): read-only while NIP46_RELAYS decides,
// otherwise staged edits saved whole, within the limits, with a warning before a removal; and the
// relay search over NIP-66 reports, with its fallback.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises, type DOMWrapper } from '@vue/test-utils'
import { readBody, setResponseStatus } from 'h3'
import { FakeRelays, nostrEvent } from '../helpers/relays'
import Section from '~/components/bunker-relays/Section.vue'
import type { BunkerRelays } from '#shared/types/bunker'
import { clearNuxtData } from '#imports'

const { holder } = vi.hoisted(() => ({ holder: { io: undefined as unknown } }))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))

/** What GET answers, and every PUT body received. */
let current: BunkerRelays
let puts: { relays: string[] }[]
let refusal: { status: number, data: { error: string, message: string } } | undefined

registerEndpoint('/api/bunker/relays', async (event) => {
  if (event.method === 'PUT') {
    const body = await readBody<{ relays: string[] }>(event)
    puts.push(body)
    // As the proxy passes the bunker's answer through: the body is { error, message }.
    if (refusal) {
      setResponseStatus(event, refusal.status)
      return refusal.data
    }
    current = { ...current, relays: body.relays.map(url => ({ url, connected: false })) }
  }
  return current
})

const console_ = (...urls: string[]): BunkerRelays => ({ source: 'console', nip46_enabled: true, relays: urls.map(url => ({ url, connected: true })) })

type Mounted = Awaited<ReturnType<typeof mountSuspended>>
type Element_ = DOMWrapper<Element>

async function mount(): Promise<Mounted> {
  const component = await mountSuspended(Section)
  await flushPromises()
  return component
}

const rows = (c: Mounted) => c.findAll('[data-testid="bunker-relays-list"] li')
const button = (c: Mounted, name: RegExp) => c.findAll('button').find((b: Element_) => name.test(b.text()))

async function addByAddress(c: Mounted, value: string) {
  await c.find('[data-testid="bunker-relays-add"] input').setValue(value)
  await c.find('[data-testid="bunker-relays-add"]').trigger('submit')
  await flushPromises()
}

async function save(c: Mounted) {
  await c.find('[data-testid="bunker-relays-save"]').trigger('click')
  await flushPromises()
}

describe('Bunker relays section', () => {
  beforeEach(async () => {
    await clearNuxtData()
    puts = []
    refusal = undefined
    holder.io = new FakeRelays()
  })

  it('is read-only while NIP46_RELAYS sets the relays: listed with status, nothing to change', async () => {
    current = { source: 'environment', nip46_enabled: true, relays: [{ url: 'wss://env-a.example', connected: true }, { url: 'wss://env-b.example', connected: false }] }
    const c = await mount()

    expect(c.find('[data-testid="bunker-relays-read-only"]').text()).toContain('NIP46_RELAYS')
    expect(rows(c).map((r: Element_) => r.attributes('data-relay'))).toEqual(['wss://env-a.example', 'wss://env-b.example'])
    expect(rows(c)[0]!.text()).toContain('Connected')
    expect(rows(c)[1]!.text()).toContain('Not connected')
    expect(c.findAll('input')).toHaveLength(0)
    expect(c.findAll('form')).toHaveLength(0)
    expect(c.findAll('button')).toHaveLength(0)
    expect(c.find('[data-testid="bunker-relays-save"]').exists()).toBe(false)
  })

  it('adds a relay by address, normalised, and saves the whole list in order', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const c = await mount()
    expect(c.find('[data-testid="bunker-relays-read-only"]').exists()).toBe(false)
    expect(c.find('[data-testid="bunker-relays-save"]').attributes('disabled')).toBeDefined()

    await addByAddress(c, '  WSS://C.example/ ')
    expect(rows(c).at(-1)!.attributes('data-pending')).toBe('add')
    expect(rows(c).at(-1)!.attributes('data-relay')).toBe('wss://c.example')

    await save(c)
    // The PUT goes through a real fetch: wait for its answer.
    await vi.waitFor(() => expect(c.find('[data-testid="bunker-relays-saved"]').exists()).toBe(true))
    expect(puts).toEqual([{ relays: ['wss://a.example', 'wss://b.example', 'wss://c.example'] }])
    expect(rows(c).every((r: Element_) => r.attributes('data-pending') === undefined)).toBe(true)
  })

  it('refuses an address the bunker would refuse, or one already listed', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const c = await mount()

    for (const [value, message] of [
      ['https://relay.example', 'start with wss://'],
      ['ws://relay.example', 'only allowed for a relay on this machine'],
      ['wss://A.example/', 'already listed']
    ] as const) {
      await addByAddress(c, value)
      expect(c.find('[data-testid="bunker-relays-add"]').text(), value).toContain(message)
    }
    await addByAddress(c, 'ws://localhost:7777')
    expect(rows(c)).toHaveLength(3)
  })

  it('warns below two relays, and refuses an empty list while NIP-46 is on', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const c = await mount()
    expect(c.find('[data-testid="bunker-relays-few"]').exists()).toBe(false)

    await button(c, /^Remove$/)!.trigger('click')
    await flushPromises()
    expect(c.find('[data-testid="bunker-relays-few"]').text()).toContain('Fewer than 2 relays')

    await button(c, /^Remove$/)!.trigger('click')
    await flushPromises()
    expect(c.find('[data-testid="bunker-relays-none"]').exists()).toBe(true)
    expect(c.find('[data-testid="bunker-relays-save"]').attributes('disabled')).toBeDefined()
  })

  it('stops at six relays', async () => {
    current = console_(...Array.from({ length: 5 }, (_, i) => `wss://r${i}.example`))
    const c = await mount()
    await addByAddress(c, 'wss://r5.example')
    expect(rows(c)).toHaveLength(6)
    expect(c.find('[data-testid="bunker-relays-add"] input').attributes('disabled')).toBeDefined()
    expect(c.find('[data-testid="bunker-relays-add"]').text()).toContain('at most 6 relays')
  })

  it('warns before saving a removal that apps through it must reconnect, then saves', async () => {
    current = console_('wss://a.example', 'wss://b.example', 'wss://c.example')
    const c = await mount()
    await c.find('[data-relay="wss://b.example"] button').trigger('click')
    await flushPromises()
    expect(c.find('[data-relay="wss://b.example"]').attributes('data-pending')).toBe('remove')

    await save(c)
    expect(puts).toEqual([])
    const warning = c.find('[data-testid="bunker-relays-confirm"]').text()
    expect(warning).toContain('wss://b.example')
    expect(warning).toContain('reconnect with a new bunker:// string')

    await save(c)
    await vi.waitFor(() => expect(c.find('[data-testid="bunker-relays-saved"]').exists()).toBe(true))
    expect(puts).toEqual([{ relays: ['wss://a.example', 'wss://c.example'] }])
    expect(c.find('[data-testid="bunker-relays-confirm"]').exists()).toBe(false)
  })

  it('undoes a removal, and shows the bunker\'s reason when it refuses', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const c = await mount()
    await c.find('[data-relay="wss://b.example"] button').trigger('click')
    await flushPromises()
    await c.find('[data-relay="wss://b.example"] button').trigger('click')
    await flushPromises()
    expect(c.find('[data-relay="wss://b.example"]').attributes('data-pending')).toBeUndefined()

    refusal = { status: 400, data: { error: 'invalid_relay_url', message: 'wss://x.example is not a relay the bunker can use.' } }
    await addByAddress(c, 'wss://x.example')
    await save(c)
    await vi.waitFor(() => expect(c.find('[data-testid="bunker-relays-save-error"]').text()).toContain('is not a relay the bunker can use'))
  })

  it('finds known relays from NIP-66 reports, filters them, and adds one', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const relays = new FakeRelays()
    relays.events.push(
      nostrEvent('m'.repeat(64), 30166, [['d', 'wss://fast.example/'], ['rtt-open', '40'], ['R', '!auth'], ['R', '!payment']], 100),
      nostrEvent('m'.repeat(64), 30166, [['d', 'wss://paid.example'], ['rtt-open', '90'], ['R', 'auth'], ['R', 'payment']], 100),
      nostrEvent('m'.repeat(64), 30166, [['d', 'ws://insecure.example'], ['rtt-open', '10']], 100),
      nostrEvent('m'.repeat(64), 30166, [['d', 'wss://a.example']], 100)
    )
    holder.io = relays
    const c = await mount()

    await c.find('[data-testid="bunker-relays-search-start"]').trigger('click')
    await flushPromises()
    expect(relays.filters[0]).toMatchObject({ kinds: [30166] })
    const results = c.find('[data-testid="bunker-relays-search-results"]')
    expect(results.findAll('li').map((li: Element_) => li.attributes('data-relay'))).toEqual(['wss://fast.example', 'wss://paid.example', 'wss://a.example'])
    expect(results.find('[data-relay="wss://fast.example"]').text()).toContain('40 ms')
    expect(results.find('[data-relay="wss://fast.example"]').text()).not.toContain('Requires auth')
    expect(results.find('[data-relay="wss://paid.example"]').text()).toContain('Requires auth')
    expect(results.find('[data-relay="wss://paid.example"]').text()).toContain('Paid')
    expect(results.find('[data-relay="wss://a.example"] button').text()).toBe('Listed')

    await c.find('input[aria-label="Filter known relays by address"]').setValue('PAID')
    await flushPromises()
    expect(c.findAll('[data-testid="bunker-relays-search-results"] li')).toHaveLength(1)

    await c.find('[data-testid="bunker-relays-search-results"] [data-relay="wss://paid.example"] button').trigger('click')
    await flushPromises()
    expect(rows(c).at(-1)!.attributes('data-relay')).toBe('wss://paid.example')
  })

  it('offers the built-in list when no monitor answers', async () => {
    current = console_('wss://a.example', 'wss://b.example')
    const relays = new FakeRelays()
    relays.down = new Set(['wss://purplepag.es/', 'wss://profiles.nostr1.com/', 'wss://relay.nos.social/'])
    holder.io = relays
    const c = await mount()

    await c.find('[data-testid="bunker-relays-search-start"]').trigger('click')
    await flushPromises()
    expect(c.find('[data-testid="bunker-relays-search-fallback"]').exists()).toBe(true)
    expect(c.findAll('[data-testid="bunker-relays-search-results"] li').length).toBeGreaterThan(0)
  })

  it('says when NIP-46 is off, and then allows an empty list', async () => {
    current = { source: 'console', nip46_enabled: false, relays: [{ url: 'wss://a.example', connected: false }] }
    const c = await mount()
    expect(c.find('[data-testid="bunker-relays-nip46-off"]').exists()).toBe(true)
    await button(c, /^Remove$/)!.trigger('click')
    await flushPromises()
    expect(c.find('[data-testid="bunker-relays-none"]').exists()).toBe(false)
    expect(c.find('[data-testid="bunker-relays-save"]').attributes('disabled')).toBeUndefined()
  })
})
