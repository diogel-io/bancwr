import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import { FakeRelays, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import Profile from '~/pages/profile.vue'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const me = 'a'.repeat(64)
const { holder } = vi.hoisted(() => ({
  holder: { io: undefined as unknown, signer: undefined as unknown }
}))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))
mockNuxtImport('useUserSigner', () => () => ({ signer: async () => holder.signer }))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))

const signer: NostrSigner = {
  signEvent: async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent
}

// The modal and popovers need the provider UApp installs in app.vue.
const InApp = defineComponent({ render: () => h(UApp, () => h(Profile)) })

let relays: FakeRelays

async function mount() {
  const component = await mountSuspended(InApp, { attachTo: document.body })
  await flushPromises()
  return component
}

/** The input a field's label points at. */
function field(component: Awaited<ReturnType<typeof mount>>, label: string) {
  const element = component.findAll('label').find(l => l.text().trim() === label)
  if (!element) throw new Error(`No field labelled ${label}`)
  return component.find(`#${element.attributes('for')}`)
}

const save = (component: Awaited<ReturnType<typeof mount>>) => component.find('[data-testid="profile-save"]').trigger('click')

describe('Profile page', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    signInAs('signer')
    relays = new FakeRelays()
    holder.io = relays
    holder.signer = signer
  })

  it('edits every field and publishes it as kind 0, keeping fields the form does not edit', async () => {
    relays.events.push(profileEvent(me, { name: 'alice', pronouns: 'they/them', website: 'https://old.example' }, 100))
    const component = await mount()
    expect((field(component, 'Name').element as HTMLInputElement).value).toBe('alice')

    const values: [string, string][] = [
      ['Name', 'alice2'], ['Display name', 'Alice'], ['About', 'Hello\nthere'], ['Website', 'https://alice.example'],
      ['Lightning address', 'alice@wallet.example'], ['NIP-05 identifier', 'alice@alice.example']
    ]
    for (const [label, value] of values) await field(component, label).setValue(value)
    await component.find('input[aria-label="Picture address"]').setValue('https://img.example/a.webp')
    await component.find('input[aria-label="Banner address"]').setValue('https://img.example/b.webp')
    await component.find('input[aria-label="Birthday year"]').setValue('1990')
    await component.find('input[aria-label="Birthday month"]').setValue('4')
    await component.find('input[aria-label="Birthday day"]').setValue('12')
    await component.find('button[role="switch"]').trigger('click')

    await save(component)
    await flushPromises()

    expect(relays.published).toHaveLength(1)
    expect(relays.published[0]!.kind).toBe(0)
    expect(relays.published[0]!.tags).toEqual([])
    expect(JSON.parse(relays.published[0]!.content)).toEqual({
      name: 'alice2', display_name: 'Alice', about: 'Hello\nthere', website: 'https://alice.example',
      picture: 'https://img.example/a.webp', banner: 'https://img.example/b.webp', nip05: 'alice@alice.example',
      lud16: 'alice@wallet.example', bot: true, birthday: { year: 1990, month: 4, day: 12 },
      pronouns: 'they/them'
    })
    expect(component.find('[data-testid="profile-saved"]').exists()).toBe(true)
  })

  it('offers to create a profile when the key has none', async () => {
    const component = await mount()
    expect(component.find('[data-testid="profile-new"]').exists()).toBe(true)
  })

  it('will not edit when no relay can be read', async () => {
    relays.down = new Set(['wss://relay.damus.io/', 'wss://nos.lol/', 'wss://relay.primal.net/'])
    const component = await mount()
    expect(component.find('[data-testid="profile-load-failed"]').exists()).toBe(true)
    expect(component.find('[data-testid="profile-save"]').exists()).toBe(false)
  })

  it('says why a save was refused, and publishes nothing', async () => {
    relays.events.push(profileEvent(me, { name: 'alice', other: 1 }, 100))
    const component = await mount()
    await field(component, 'Name').setValue('changed')
    relays.down = new Set(['wss://relay.damus.io/', 'wss://nos.lol/', 'wss://relay.primal.net/'])
    await save(component)
    await flushPromises()
    expect(component.find('[data-testid="profile-save-error"]').text()).toContain('could lose fields set in other apps')
    expect(relays.published).toEqual([])
  })

  it('holds back an invalid form', async () => {
    const component = await mount()
    await field(component, 'Website').setValue('not a url')
    expect(component.text()).toContain('Enter a full address starting with https://')
    expect(component.find('[data-testid="profile-save"]').attributes('disabled')).toBeDefined()
  })

  it('asks the member to connect their signer when it is not available', async () => {
    holder.signer = undefined
    const component = await mount()
    await field(component, 'Name').setValue('alice')
    await save(component)
    await flushPromises()
    expect(document.body.textContent).toContain('Connect your signer')
    expect(relays.published).toEqual([])

    // Closing the dialog cancels, saying so, rather than leaving the save waiting.
    ;(document.body.querySelector('[role="dialog"] button[aria-label="Close"]') as HTMLButtonElement).click()
    await flushPromises()
    expect(component.find('[data-testid="profile-save-error"]').text()).toContain('no signer was connected')
  })

  it('is one column on small screens, two on large ones', async () => {
    const component = await mount()
    const classes = component.find('[data-testid="profile-layout"]').classes()
    expect(classes).toContain('grid-cols-1')
    expect(classes.some(c => c.startsWith('lg:grid-cols-'))).toBe(true)
  })
})
