import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { createError } from 'h3'
import { npubEncode } from 'nostr-tools/nip19'
import { FakeRelays, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import Member from '~/pages/team/[pubkey].vue'
import { clearNuxtData } from '#imports'

// A team member's read-only profile (#77): who the vault says they are, and their kind 0 from
// their relays, read as the profile page reads one and never published.
const bob = 'b'.repeat(64)
const bobNpub = npubEncode(bob)
const stranger = 'c'.repeat(64)
const { holder } = vi.hoisted(() => ({ holder: { io: undefined as unknown } }))
vi.mock('~/utils/relay-io', () => ({ createRelayIO: () => holder.io }))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint(`/api/bunker/team/by-pubkey/${bob}`, () => ({ id: '2', name: 'Bob', pubkey: bob, npub: bobNpub, role: 'signer' }))
registerEndpoint(`/api/bunker/team/by-pubkey/${stranger}`, () => {
  throw createError({ status: 404, data: { error: 'not_registered' } })
})

let relays: FakeRelays

async function open(path: string) {
  const component = await mountSuspended(Member, { route: path })
  await flushPromises()
  return component
}

describe('Team member page', () => {
  beforeEach(async () => {
    await clearNuxtData()
    signInAs('viewer')
    relays = new FakeRelays()
    holder.io = relays
  })

  it('shows the member\'s name, role and profile read-only', async () => {
    relays.events.push(profileEvent(bob, {
      name: 'bob', display_name: 'Bob B', about: 'Hello from Bob', picture: 'https://img.example/bob.png',
      nip05: 'bob@bob.example', website: 'https://bob.example'
    }, 100))
    const component = await open(`/team/${bobNpub}`)

    expect(component.find('[data-testid="member-summary"]').text()).toContain('Bob')
    expect(component.find('[data-testid="member-role"]').text()).toBe('Signer')
    const profile = component.find('[data-testid="member-profile"]')
    expect(profile.text()).toContain('Bob B')
    expect(profile.text()).toContain('Hello from Bob')
    expect(profile.text()).toContain('bob@bob.example')
    expect(profile.text()).toContain('https://bob.example')
    expect(profile.find('img[src="https://img.example/bob.png"]').exists()).toBe(true)
    // Read-only: nothing to edit, nothing published.
    expect(component.findAll('input, textarea')).toHaveLength(0)
    expect(component.text()).not.toContain('Save')
    expect(relays.published).toEqual([])
    expect(relays.filters).toContainEqual({ kinds: [0], authors: [bob] })
  })

  it('takes a hex key as well as an npub', async () => {
    relays.events.push(profileEvent(bob, { name: 'bob' }, 100))
    const component = await open(`/team/${bob}`)
    expect(component.find('[data-testid="member-profile"]').text()).toContain('bob')
  })

  it('says when no relay holds a profile', async () => {
    const component = await open(`/team/${bobNpub}`)
    expect(component.find('[data-testid="member-profile-not-found"]').exists()).toBe(true)
  })

  it('says when no relay answers', async () => {
    relays.query = async (urls: string[]) => ({ events: [], reached: [], failed: urls })
    const component = await open(`/team/${bobNpub}`)
    expect(component.find('[data-testid="member-profile-failed"]').exists()).toBe(true)
  })

  it('shows no profile for a key that is not in the vault, and never reads relays for it', async () => {
    const component = await open(`/team/${npubEncode(stranger)}`)
    expect(component.find('[data-testid="member-not-registered"]').exists()).toBe(true)
    expect(component.find('[data-testid="member-profile"]').exists()).toBe(false)
    expect(relays.queries).toBe(0)
  })

  it('says when the address is not a key', async () => {
    const component = await open('/team/not-a-key')
    expect(component.find('[data-testid="member-invalid-key"]').exists()).toBe(true)
    expect(relays.queries).toBe(0)
  })

  it('links back to the team', async () => {
    const component = await open(`/team/${bobNpub}`)
    expect(component.findAll('a').map(a => a.attributes('href'))).toContain('/team')
  })
})
