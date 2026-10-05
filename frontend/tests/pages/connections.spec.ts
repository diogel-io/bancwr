import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createError, readBody, setResponseStatus } from 'h3'
import { UApp } from '#components'
import type { Nip46Connection, Nip46Token, Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Connections from '~/pages/connections.vue'
import { clearNuxtData } from '#imports'

// signInAs() signs in as 'a'.repeat(64).
const me = 'a'.repeat(64)
const other = 'b'.repeat(64)

const connection = (over: Partial<Nip46Connection> = {}): Nip46Connection => ({
  id: 'c1', client_pubkey: 'c'.repeat(64), for_pubkey: me, client_name: 'Damus', client_url: 'https://damus.io', client_image: null,
  metadata_verified: false, kinds: [1, 7], connected_at: new Date(Date.now() - 3_600_000).toISOString(),
  last_used_at: new Date(Date.now() - 120_000).toISOString(), revoked_at: null, revoked_reason: null, revoked_by: null, ...over
})

const state = vi.hoisted(() => ({
  connections: [] as unknown[],
  tokens: [] as unknown[],
  listStatus: 200,
  issueStatus: 200,
  deleted: [] as string[],
  issued: [] as unknown[]
}))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint('/api/bunker/connections', () => {
  if (state.listStatus !== 200) throw createError({ status: state.listStatus, data: { error: 'forbidden' } })
  return state.connections
})
registerEndpoint('/api/bunker/connections/tokens', {
  method: 'GET',
  handler: () => state.tokens
})
registerEndpoint('/api/bunker/connections/tokens', {
  method: 'POST',
  handler: async (event) => {
    // As the proxy passes the bunker's answer through: the body is { error, message }.
    if (state.issueStatus === 409) {
      setResponseStatus(event, 409)
      return { error: 'nip46_disabled', message: 'off' }
    }
    state.issued.push(await readBody(event))
    return { id: 't9', uri: `bunker://${'e'.repeat(64)}?relay=wss%3A%2F%2Frelay.example&secret=${'f'.repeat(64)}`, for_pubkey: me, kinds: [1, 7], expires_at: new Date(Date.now() + 86_400_000).toISOString() }
  }
})
registerEndpoint('/api/bunker/connections/c1', {
  method: 'DELETE',
  handler: () => {
    state.deleted.push('c1')
    state.connections = state.connections.map(c => ({ ...(c as Nip46Connection), revoked_at: new Date().toISOString(), revoked_reason: 'revoked', revoked_by: me }))
    return { success: true }
  }
})
registerEndpoint('/api/bunker/team', () => [
  { id: '1', name: 'Gary', pubkey: me, npub: 'npub1me', role: 'administrator' },
  { id: '2', name: 'Bob', pubkey: other, npub: 'npub1bob', role: 'signer' },
  { id: '3', name: 'Vera', pubkey: 'd'.repeat(64), npub: 'npub1vera', role: 'viewer' }
])

const InApp = defineComponent({ render: () => h(UApp, () => h(Connections)) })

// Unmounted after each test: they share useFetch keys, and one left mounted re-renders on the next
// test's data.
let mounted: { unmount: () => void }[] = []

async function mount(role: Role) {
  signInAs(role)
  const component = await mountSuspended(InApp, { attachTo: document.body })
  mounted.push(component)
  await flushPromises()
  return component
}

describe('Connections page', () => {
  afterEach(() => {
    for (const component of mounted) component.unmount()
    mounted = []
  })

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    Object.assign(state, { connections: [], tokens: [], listStatus: 200, issueStatus: 200, deleted: [], issued: [] })
  })

  it('always explains that apps sign as the bunker\'s key', async () => {
    const component = await mount('signer')
    expect(component.find('[data-testid="connections-explainer"]').text()).toContain('sign as this bunker\'s key, not your own')
  })

  it('lists a signer\'s apps with their unverified name, kinds and times, and no issuing', async () => {
    state.connections = [connection()]
    const component = await mount('signer')
    const row = component.find('[data-connection="c1"]')
    expect(row.text()).toContain('Damus')
    expect(row.find('[data-testid="unverified"]').exists()).toBe(true)
    expect(row.text()).toContain('Says it is https://damus.io')
    expect(row.text()).toContain('1 Note')
    expect(row.text()).toContain('7 Reaction')
    expect(row.text()).toContain('Connected 1 hour ago')
    expect(row.text()).toContain('last signed 2 minutes ago')
    expect(row.text()).not.toContain('for ')
    expect(component.find('[data-testid="issue-token"]').exists()).toBe(false)
  })

  it('shows an app that gave no name by its key, without the unverified badge', async () => {
    state.connections = [connection({ client_name: null, client_url: null })]
    const component = await mount('signer')
    const row = component.find('[data-connection="c1"]')
    expect(row.text()).toContain('npub1')
    expect(row.find('[data-testid="unverified"]').exists()).toBe(false)
  })

  it('revokes only after confirming, and shows who ended it', async () => {
    state.connections = [connection()]
    const component = await mount('signer')
    const revoke = () => component.find('[data-connection="c1"]').findAll('button').find(b => b.text() === 'Revoke')!
    await revoke().trigger('click')
    expect(state.deleted).toEqual([])
    expect(component.find('[data-connection="c1"]').text()).toContain('Revoke this app?')
    await revoke().trigger('click')
    await vi.waitFor(() => expect(state.deleted).toEqual(['c1']))
    // Ended connections are hidden until asked for.
    await vi.waitFor(() => expect(component.find('[data-connection="c1"]').exists()).toBe(false))
    await component.find('[role="switch"]').trigger('click')
    expect(component.find('[data-connection="c1"]').text()).toContain('Revoked by you')
  })

  it('shows the empty state, for a signer and for an administrator', async () => {
    const user = await mount('signer')
    expect(user.find('[data-testid="connections-empty"]').text()).toContain('No apps are connected for you')
    expect(user.find('[data-testid="connections-empty"]').text()).toContain('Ask an administrator')
    user.unmount()
    mounted = []
    await clearNuxtData()

    document.body.innerHTML = ''
    const admin = await mount('administrator')
    expect(admin.find('[data-testid="connections-empty"]').text()).toContain('Issue a connection token below')
  })

  it('shows an administrator whom each app is for, and unused tokens', async () => {
    state.connections = [connection({ for_pubkey: other })]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Bob\'s laptop', kinds: [1], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null } satisfies Nip46Token]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-connection="c1"]').text()).toContain('for Bob')
    expect(component.find('[data-token="t1"]').text()).toContain('Bob\'s laptop')
  })

  it('issues a token, shows the string once with a QR code, and forgets it on Done', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Damus on my phone')
    await form.trigger('submit')
    await vi.waitFor(() => expect(component.find('[data-testid="issued-uri"]').exists(), component.text()).toBe(true))

    expect(state.issued).toEqual([{ for_pubkey: me, label: 'Damus on my phone', kinds: [1, 7], expires_in_hours: 24 }])
    expect(component.find('[data-testid="issued-uri"]').text()).toContain('secret=')
    expect(component.find('[data-testid="issued-qr"]').attributes('src')).toMatch(/^data:image\/svg\+xml/)
    expect(component.text()).toContain('Shown once')

    await component.findAll('button').find(b => b.text() === 'Done')!.trigger('click')
    expect(component.find('[data-testid="issued-uri"]').exists()).toBe(false)
    expect(component.text()).not.toContain('secret=')
  })

  it('offers tokens only for admins and signers, never a viewer (#77)', async () => {
    const component = await mount('administrator')
    const forSelect = component.find('[data-testid="issue-token"]').find('button[role="combobox"]')
    await forSelect.trigger('click')
    await flushPromises()
    const options = [...document.body.querySelectorAll('[role="option"]')].map(o => o.textContent?.trim())
    expect(options).toEqual(expect.arrayContaining(['Gary (you)', 'Bob']))
    expect(options).not.toContain('Vera')
  })

  it('explains that NIP-46 is off when the bunker refuses to issue', async () => {
    state.issueStatus = 409
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('x')
    await form.trigger('submit')
    await flushPromises()
    expect(component.find('[data-testid="issue-error"]').text()).toContain('NIP46_ENABLED=true')
  })

  it('shows the forbidden notice when the bunker refuses the list', async () => {
    state.listStatus = 403
    const component = await mount('signer')
    expect(component.find('[data-testid="forbidden-notice"]').exists()).toBe(true)
  })
})
