import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createError, readBody } from 'h3'
import { UApp } from '#components'
import type { BunkerIdentity, Nip46Connection, Nip46Token, Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Connections from '~/pages/connections.vue'
import { clearNuxtData } from '#imports'

// signInAs() signs in as 'a'.repeat(64).
const me = 'a'.repeat(64)
const other = 'b'.repeat(64)

const connection = (over: Partial<Nip46Connection> = {}): Nip46Connection => ({
  id: 'c1', client_pubkey: 'c'.repeat(64), for_pubkey: me, client_name: 'Damus', client_url: 'https://damus.io', client_image: null,
  metadata_verified: false, kinds: [1, 7], methods: [], connected_at: new Date(Date.now() - 3_600_000).toISOString(),
  last_used_at: new Date(Date.now() - 120_000).toISOString(), revoked_at: null, revoked_reason: null, revoked_by: null, expires_at: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [], ...over
})

const state = vi.hoisted(() => ({
  connections: [] as unknown[],
  tokens: [] as unknown[],
  listStatus: 200,
  issueStatus: 200,
  deleted: [] as string[],
  issued: [] as unknown[],
  identities: [] as unknown[]
}))

registerEndpoint('/api/bunker/identities', () => state.identities)

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
    // As the proxy rethrows the bunker's refusal: an H3 error, with the bunker's { error, message }
    // under its data and only the status text as its own message.
    if (state.issueStatus === 409) {
      throw createError({ statusCode: 409, statusMessage: 'Conflict', data: { error: 'nip46_disabled', message: 'off' } })
    }
    const body = await readBody(event)
    state.issued.push(body)
    // As the bunker answers: the host and identity_pubkey are the identity it signs as, the
    // primary ('e'.repeat(64) here) unless one was chosen.
    const identity = (body as { identity?: string }).identity ?? 'e'.repeat(64)
    return { id: 't9', uri: `bunker://${identity}?relay=wss%3A%2F%2Frelay.example&secret=${'f'.repeat(64)}`, for_pubkey: me, kinds: [1, 7], methods: [], connection_lifetime_hours: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [], expires_at: new Date(Date.now() + 86_400_000).toISOString(), identity_pubkey: identity }
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
    Object.assign(state, { connections: [], tokens: [], listStatus: 200, issueStatus: 200, deleted: [], issued: [], identities: [] })
  })

  it('always explains that apps sign as the bunker\'s key', async () => {
    const component = await mount('signer')
    expect(component.find('[data-testid="connections-explainer"]').text()).toContain('sign as this bunker\'s key, not your own')
  })

  it('lists a signer\'s apps with their unverified name, kinds and times', async () => {
    state.connections = [connection()]
    const component = await mount('signer')
    const row = component.find('[data-connection="c1"]')
    expect(row.text()).toContain('Damus')
    expect(row.find('[data-testid="unverified"]').exists()).toBe(true)
    expect(row.text()).toContain('Says it is https://damus.io')
    expect(row.text()).toContain('1 Note')
    expect(row.text()).toContain('7 Reaction')
    expect(row.text()).toContain('Connected 1 hour ago')
    expect(row.text()).toContain('last used 2 minutes ago')
    expect(row.text()).not.toContain('for ')
  })

  it('lets a signer issue their own token, with no member select or approval (#142)', async () => {
    const component = await mount('signer')
    const form = component.find('[data-testid="issue-token"]')
    expect(form.exists()).toBe(true)
    expect(form.find('#issue-token-for').exists()).toBe(false)
    expect(form.find('[data-testid="issue-token-approval"]').exists()).toBe(false)
    await form.find('input[maxlength="100"]').setValue('My phone')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).toMatchObject({ for_pubkey: me, label: 'My phone' })
    expect(state.issued[0]).not.toHaveProperty('approval_kinds')
    await vi.waitFor(() => expect(component.find('[data-testid="issued-uri"]').exists()).toBe(true))
  })

  it('offers a signer every identity they are granted, as the bunker lists them (#142)', async () => {
    const brand = '9'.repeat(64)
    // A signer's list is only those they may use, without members.
    state.identities = [
      { pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Primary', is_primary: true, available: true },
      { pubkey: brand, npub: 'npub1brand', label: 'Brand B', is_primary: false, available: true }
    ] satisfies BunkerIdentity[]
    const component = await mount('signer')
    await flushPromises()
    const form = component.find('[data-testid="issue-token"]')
    await form.find('#issue-token-identity').trigger('click')
    await flushPromises()
    const offered = [...document.body.querySelectorAll('[role="option"]')].map(o => o.textContent?.trim())
    expect(offered).toEqual(['Primary', 'Brand B'])
  })

  it('lists a signer\'s own unused tokens (#142)', async () => {
    state.tokens = [{ id: 't1', for_pubkey: me, issued_by: me, label: 'Mine', kinds: [1], methods: [], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [] } satisfies Nip46Token]
    const component = await mount('signer')
    await flushPromises()
    expect(component.find('[data-token="t1"]').text()).toContain('Mine')
    expect(component.find('[data-token="t1"]').text()).toContain('For you')
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
    expect(user.find('[data-testid="connections-empty"]').text()).toContain('Issue a connection token below')
    user.unmount()
    mounted = []
    await clearNuxtData()

    document.body.innerHTML = ''
    const admin = await mount('administrator')
    expect(admin.find('[data-testid="connections-empty"]').text()).toContain('Issue a connection token below')
  })

  it('shows an administrator whom each app is for, and unused tokens', async () => {
    state.connections = [connection({ for_pubkey: other })]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Bob\'s laptop', kinds: [1], methods: [], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [] } satisfies Nip46Token]
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

    expect(state.issued).toEqual([{ for_pubkey: me, label: 'Damus on my phone', kinds: [1, 7], methods: [], expires_in_hours: 24 }])
    expect(component.find('[data-testid="issued-uri"]').text()).toContain('secret=')
    expect(component.find('[data-testid="issued-qr"]').attributes('src')).toMatch(/^data:image\/svg\+xml/)
    expect(component.text()).toContain('Shown once')
    // With no identities listed, the result still names the key, as an npub (#137).
    expect(component.find('[data-testid="issued-identity"]').text()).toMatch(/^Signs as npub1[0-9a-z]+$/)

    await component.findAll('button').find(b => b.text() === 'Done')!.trigger('click')
    expect(component.find('[data-testid="issued-uri"]').exists()).toBe(false)
    expect(component.text()).not.toContain('secret=')
  })

  it('issues a token with encryption methods, in the bunker\'s order, and warns about decrypting (#97)', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Messages app')
    expect(component.find('[data-testid="issue-token-decrypt-warning"]').exists()).toBe(false)

    await form.find('[data-method="nip44_decrypt"]').trigger('click')
    await form.find('[data-method="nip44_encrypt"]').trigger('click')
    await flushPromises()
    expect(component.find('[data-testid="issue-token-decrypt-warning"]').text()).toContain('every message sent to the brand\'s key')

    await form.trigger('submit')
    await vi.waitFor(() => expect(component.find('[data-testid="issued-uri"]').exists(), component.text()).toBe(true))
    expect(state.issued).toEqual([{ for_pubkey: me, label: 'Messages app', kinds: [1, 7], methods: ['nip44_encrypt', 'nip44_decrypt'], expires_in_hours: 24 }])
  })

  it('issues an encryption-only token, but not one with nothing allowed', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Encrypt only')
    await form.find('#issue-token-kinds').trigger('click')
    await flushPromises()
    const none = [...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.includes('encryption only')) as HTMLElement
    // reka-ui selects an option on pointerup.
    none.dispatchEvent(new Event('pointerup', { bubbles: true }))
    await vi.waitFor(() => expect(component.find('[data-testid="issue-token-methods"]').text()).toContain('Choose at least one method'))
    const submit = () => component.findAll('button').find(b => b.text() === 'Issue token')!
    expect(submit().attributes('disabled')).toBeDefined()

    await form.find('[data-method="nip04_encrypt"]').trigger('click')
    await flushPromises()
    expect(submit().attributes('disabled')).toBeUndefined()
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).toMatchObject({ kinds: [], methods: ['nip04_encrypt'] })
  })

  it('lists the methods a token and a connection allow', async () => {
    state.connections = [connection({ methods: ['nip44_encrypt', 'nip44_decrypt'] })]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Reader', kinds: [], methods: ['nip44_decrypt'], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [] } satisfies Nip46Token]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-testid="connection-methods"]').text()).toContain('Encrypt messages (NIP-44)')
    expect(component.find('[data-testid="connection-methods"]').text()).toContain('Read encrypted messages (NIP-44)')
    const token = component.find('[data-token="t1"]').text()
    expect(token).toContain('may also: Read encrypted messages (NIP-44)')
    expect(token).not.toContain('may sign')
  })

  it('issues a token whose connection lasts 30 days, and lists it with its end (#98)', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Contractor')
    await form.find('#issue-token-lifetime').trigger('click')
    await flushPromises()
    const option = [...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.trim() === '30 days') as HTMLElement
    // reka-ui selects an option on pointerup.
    option.dispatchEvent(new Event('pointerup', { bubbles: true }))
    await vi.waitFor(() => expect(form.find('#issue-token-lifetime').text()).toContain('30 days'))
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).toMatchObject({ label: 'Contractor', connection_lifetime_hours: 720 })
  })

  it('issues a token with a rate limit, and the default sends none (#101)', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Phone')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).not.toHaveProperty('rate_per_minute')
    expect(state.issued[0]).not.toHaveProperty('rate_burst')

    await component.find('[data-testid="issued-token"]').findAll('button').find(b => b.text() === 'Done')!.trigger('click')
    await flushPromises()
    const again = component.find('[data-testid="issue-token"]')
    await again.find('input[maxlength="100"]').setValue('Bot')
    await again.find('#issue-token-rate-limit').trigger('click')
    await flushPromises()
    const option = [...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.trim() === '10 a minute (burst 5)') as HTMLElement
    option.dispatchEvent(new Event('pointerup', { bubbles: true }))
    await vi.waitFor(() => expect(again.find('#issue-token-rate-limit').text()).toContain('10 a minute'))
    await again.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(2))
    expect(state.issued[1]).toMatchObject({ label: 'Bot', rate_per_minute: 10, rate_burst: 5 })
  })

  it('issues a token whose notes need approval, and shows it on tokens and connections (#102)', async () => {
    state.connections = [connection({ id: 'c1', approval_kinds: [1] })]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Bot', kinds: [1, 7], methods: [], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: null, rate_per_minute: 60, rate_burst: 20, approval_kinds: [1] } satisfies Nip46Token]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-connection="c1"] [data-testid="connection-approval"]').text()).toBe('needs approval: 1 Note')
    expect(component.find('[data-token="t1"] [data-testid="token-approval"]').text()).toBe('needs approval: 1 Note')

    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Scheduler')
    // The default preset is notes and reactions; only notes need approval.
    const approval = form.find('[data-testid="issue-token-approval"]')
    expect(approval.findAll('[data-kind]').map(c => c.attributes('data-kind'))).toEqual(['1', '7'])
    await approval.find('[data-kind="1"] button, button[data-kind="1"], #issue-token-approval-1').trigger('click')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).toMatchObject({ label: 'Scheduler', kinds: [1, 7], approval_kinds: [1] })
  })

  it('sends no approval kinds when none are chosen (#102)', async () => {
    const component = await mount('administrator')
    const form = component.find('[data-testid="issue-token"]')
    await form.find('input[maxlength="100"]').setValue('Phone')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).not.toHaveProperty('approval_kinds')
  })

  it('offers no identity choice on a bunker with one identity (#103)', async () => {
    state.identities = [{ pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Primary', is_primary: true, available: true, members: [me] } satisfies BunkerIdentity]
    state.connections = [connection()]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-testid="issue-token-identity"]').exists()).toBe(false)
    expect(component.find('[data-testid="connection-identity"]').exists()).toBe(false)
  })

  it('names the identity a token signs as on a bunker with one, for any member, before and after issuing (#137)', async () => {
    // Bob is a signer with no grant: on a bunker with only the primary, every member may use it.
    state.identities = [{ pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Bunker Admin', is_primary: true, available: true, members: [] } satisfies BunkerIdentity]
    const component = await mount('administrator')
    await flushPromises()
    const form = component.find('[data-testid="issue-token"]')
    expect(form.find('[data-testid="issue-token-signs-as"]').text()).toBe('Signs as Bunker Admin (npub1e)')

    await form.find('#issue-token-for').trigger('click')
    await flushPromises()
    ;([...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.trim() === 'Bob') as HTMLElement).dispatchEvent(new Event('pointerup', { bubbles: true }))
    await flushPromises()
    // The member chosen does not change the key that signs.
    expect(form.find('[data-testid="issue-token-signs-as"]').text()).toBe('Signs as Bunker Admin (npub1e)')
    expect(form.find('#issue-token-identity').exists()).toBe(false)

    await form.find('input[maxlength="100"]').setValue('Primal for Bob')
    await form.trigger('submit')
    await vi.waitFor(() => expect(component.find('[data-testid="issued-uri"]').exists(), component.text()).toBe(true))
    expect(state.issued[0]).toMatchObject({ for_pubkey: other })
    expect(state.issued[0]).not.toHaveProperty('identity')
    expect(component.find('[data-testid="issued-identity"]').text()).toBe('Signs as Bunker Admin (npub1e)')
  })

  it('says the member chosen under For is not the key that signs (#137)', async () => {
    const component = await mount('administrator')
    await flushPromises()
    const form = component.find('[data-testid="issue-token"]')
    expect(form.text()).toContain('The member this token is issued to.')
    expect(form.text()).toContain('The app does not sign as them.')
    expect(form.text()).not.toContain('It signs as the bunker\'s key')
  })

  it('claims no key for a member granted none of several identities (#137)', async () => {
    state.identities = [
      { pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Primary', is_primary: true, available: true, members: [me] },
      { pubkey: '9'.repeat(64), npub: 'npub1brand', label: 'Brand B', is_primary: false, available: true, members: [] }
    ] satisfies BunkerIdentity[]
    const component = await mount('administrator')
    await flushPromises()
    const form = component.find('[data-testid="issue-token"]')
    await form.find('#issue-token-for').trigger('click')
    await flushPromises()
    ;([...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.trim() === 'Bob') as HTMLElement).dispatchEvent(new Event('pointerup', { bubbles: true }))
    await flushPromises()
    const signsAs = form.find('[data-testid="issue-token-signs-as"]').text()
    expect(signsAs).toContain('may not use any of this bunker\'s identities')
    expect(signsAs).not.toContain('Primary')
  })

  it('issues for the identity chosen, offering only those the member may use, and names it on rows (#103)', async () => {
    const brand = '9'.repeat(64)
    state.identities = [
      { pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Primary', is_primary: true, available: true, members: [me] },
      { pubkey: brand, npub: 'npub1brand', label: 'Brand B', is_primary: false, available: true, members: [other] },
      { pubkey: '8'.repeat(64), npub: 'npub1c', label: 'Brand C', is_primary: false, available: true, members: [] }
    ] satisfies BunkerIdentity[]
    state.connections = [connection({ identity_pubkey: brand })]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-connection="c1"] [data-testid="connection-identity"]').text()).toBe('signs as Brand B')

    const form = component.find('[data-testid="issue-token"]')
    // For Bob, a signer granted Brand B only.
    await form.find('#issue-token-for').trigger('click')
    await flushPromises()
    ;([...document.body.querySelectorAll('[role="option"]')].find(o => o.textContent?.trim() === 'Bob') as HTMLElement).dispatchEvent(new Event('pointerup', { bubbles: true }))
    await flushPromises()
    await form.find('#issue-token-identity').trigger('click')
    await flushPromises()
    const offered = [...document.body.querySelectorAll('[role="option"]')].map(o => o.textContent?.trim())
    expect(offered).toEqual(['Brand B'])
    ;([...document.body.querySelectorAll('[role="option"]')][0] as HTMLElement).dispatchEvent(new Event('pointerup', { bubbles: true }))
    await flushPromises()
    // The form names the chosen identity's key before issuing (#137).
    expect(form.find('[data-testid="issue-token-signs-as"]').text()).toBe('Signs as Brand B (npub1brand)')
    await form.find('input[maxlength="100"]').setValue('Bob on Brand B')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.issued).toHaveLength(1))
    expect(state.issued[0]).toMatchObject({ for_pubkey: other, identity: brand })
    // And the result names the identity the response says it signs as.
    await vi.waitFor(() => expect(component.find('[data-testid="issued-identity"]').exists()).toBe(true))
    expect(component.find('[data-testid="issued-identity"]').text()).toBe('Signs as Brand B (npub1brand)')
  })

  it('shows the rate limit of each token and active connection (#101)', async () => {
    state.connections = [connection({ id: 'c1', rate_per_minute: 30, rate_burst: 10 })]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Bot', kinds: [1], methods: [], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: null, rate_per_minute: 600, rate_burst: 100, approval_kinds: [] } satisfies Nip46Token]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-connection="c1"] [data-testid="connection-rate-limit"]').text()).toBe('limited to 30 a minute (burst 10)')
    expect(component.find('[data-token="t1"] [data-testid="token-rate-limit"]').text()).toBe('limited to 600 a minute (burst 100)')
  })

  it('shows when each active connection ends, and why an expired one ended', async () => {
    state.connections = [
      connection({ id: 'c1', expires_at: new Date(Date.now() + 6 * 86_400_000).toISOString() }),
      connection({ id: 'c2', client_pubkey: 'd'.repeat(64) }),
      connection({ id: 'c3', client_pubkey: 'e'.repeat(64), expires_at: new Date(Date.now() - 3_600_000).toISOString(), revoked_at: new Date(Date.now() - 3_600_000).toISOString(), revoked_reason: 'expired' })
    ]
    state.tokens = [{ id: 't1', for_pubkey: other, issued_by: me, label: 'Bot', kinds: [1], methods: [], created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), used_at: null, revoked_at: null, connection_lifetime_hours: 168, rate_per_minute: 60, rate_burst: 20, approval_kinds: [] } satisfies Nip46Token]
    const component = await mount('administrator')
    await flushPromises()
    expect(component.find('[data-connection="c1"] [data-testid="connection-end"]').text()).toBe('ends in 6 days')
    expect(component.find('[data-connection="c2"] [data-testid="connection-end"]').text()).toBe('no time limit')
    expect(component.find('[data-token="t1"]').text()).toContain('connection lasts 7 days')

    expect(component.find('[data-connection="c3"]').exists()).toBe(false)
    await component.find('[role="switch"]').trigger('click')
    await flushPromises()
    const ended = component.find('[data-connection="c3"]')
    expect(ended.text()).toContain('Its time limit ran out')
    expect(ended.find('[data-testid="connection-end"]').exists()).toBe(false)
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
