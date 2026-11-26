// Identities in the console (#103): the Config section that lists, renames, creates and retires
// them (#146), the team page's
// "May sign as" for a member, the add-member form's choice, and the log's "Signed as" column, each
// shown only once the bunker holds more than one identity.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createError, getQuery, readBody } from 'h3'
import { UApp } from '#components'
import type { BunkerIdentity, LogEntry } from '#shared/types/bunker'
import IdentitiesSection from '~/components/identities/IdentitiesSection.vue'
import IdentityAccess from '~/components/identities/IdentityAccess.vue'
import ActivityLog from '~/components/ActivityLog.vue'
import Team from '~/pages/team/index.vue'
import { signInAs } from '../helpers/session'
import { clearNuxtData } from '#imports'

const PRIMARY = 'e'.repeat(64)
const BRAND = '9'.repeat(64)
const SAM = '5'.repeat(64)

const state = vi.hoisted(() => ({
  identities: [] as unknown[],
  calls: [] as { method: string, path: string, body?: unknown }[],
  createRefusal: undefined as string | undefined
}))

const two = (): BunkerIdentity[] => [
  { pubkey: PRIMARY, npub: 'npub1primary', label: 'Primary', is_primary: true, available: true, members: [SAM] },
  { pubkey: BRAND, npub: 'npub1brand', label: 'Brand B', is_primary: false, available: false, members: [] }
]

// As the bunker lists them: a retired identity only with `?retired=true` (#146).
registerEndpoint('/api/bunker/identities', {
  method: 'GET',
  handler: event => getQuery(event).retired === 'true'
    ? state.identities
    : (state.identities as BunkerIdentity[]).filter(i => !i.retired_at)
})
registerEndpoint('/api/bunker/identities', {
  method: 'POST',
  handler: async (event) => {
    const body = await readBody(event)
    state.calls.push({ method: 'POST', path: 'identities', body })
    if (state.createRefusal) {
      throw createError({ statusCode: 503, data: { error: state.createRefusal } })
    }
    return { pubkey: 'c'.repeat(64), npub: 'npub1sales', label: body.label, is_primary: false, available: true, members: [], usage: { connections: 0, tokens: 0 } }
  }
})
registerEndpoint(`/api/bunker/identities/${BRAND}`, {
  method: 'PATCH',
  handler: async (event) => {
    state.calls.push({ method: 'PATCH', path: BRAND, body: await readBody(event) })
    return { success: true }
  }
})
registerEndpoint(`/api/bunker/identities/${BRAND}`, {
  method: 'DELETE',
  handler: () => {
    state.calls.push({ method: 'DELETE', path: BRAND })
    return { success: true, connections_revoked: 2, tokens_revoked: 1 }
  }
})
for (const method of ['PUT', 'DELETE'] as const) {
  registerEndpoint(`/api/bunker/identities/${BRAND}/members/${SAM}`, {
    method,
    handler: () => {
      state.calls.push({ method, path: `${BRAND}/${SAM}` })
      return { success: true }
    }
  })
}
registerEndpoint('/api/bunker/team', {
  method: 'GET',
  handler: () => []
})
registerEndpoint('/api/bunker/team', {
  method: 'POST',
  handler: async (event) => {
    state.calls.push({ method: 'POST', path: 'team', body: await readBody(event) })
    return { success: true, message: 'ok' }
  }
})

const inApp = (component: unknown, props: Record<string, unknown> = {}) =>
  defineComponent({ render: () => h(UApp, () => h(component as never, props)) })

// Unmounted after each test: they share the identities' useFetch key.
let mounted: { unmount: () => void }[] = []
async function mount(component: ReturnType<typeof defineComponent> | typeof ActivityLog, options: Record<string, unknown> = {}) {
  const wrapper = await mountSuspended(component as never, options)
  mounted.push(wrapper)
  await flushPromises()
  return wrapper
}

describe('Identities', () => {
  afterEach(() => {
    for (const wrapper of mounted) wrapper.unmount()
    mounted = []
  })

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    signInAs('administrator')
    Object.assign(state, { identities: [], calls: [], createRefusal: undefined })
  })

  it('lists each identity on the Config page, marks the primary and one not loaded yet, and renames', async () => {
    state.identities = two()
    const component = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    const rows = component.findAll('[data-identity]')
    expect(rows.map(r => r.attributes('data-identity'))).toEqual([PRIMARY, BRAND])
    expect(rows[0]!.text()).toContain('Primary')
    expect(rows[1]!.find('[data-testid="identity-not-loaded"]').text()).toBe('Loads when the bunker restarts')
    expect(component.text()).toContain('bunker import-key --add')

    await rows[1]!.find('button').trigger('click')
    await component.find(`[data-identity="${BRAND}"] input`).setValue('Brand Bee')
    await component.findAll(`[data-identity="${BRAND}"] button`).find(b => b.text() === 'Save')!.trigger('click')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'PATCH', path: BRAND, body: { label: 'Brand Bee' } }]))
  })

  it('creates an identity from a label alone, and says to take a backup', async () => {
    state.identities = two()
    const component = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    const form = component.find('[data-testid="identity-create"]')
    expect(form.text()).toContain('never shown')
    await form.find('#identity-create-label').setValue('  Sales ')
    await form.trigger('submit')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'POST', path: 'identities', body: { label: 'Sales' } }]))
    await vi.waitFor(() => expect(component.find('[data-testid="identity-created"]').text()).toContain('take a backup now'))
  })

  it('says when the root credential could not be read, and that nothing changed', async () => {
    state.identities = two()
    state.createRefusal = 'root_key_unavailable'
    const component = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    await component.find('#identity-create-label').setValue('Sales')
    await component.find('[data-testid="identity-create"]').trigger('submit')
    await vi.waitFor(() => expect(component.find('[data-testid="identities-error"]').text()).toContain('BANCWR_ROOT_KEY_FILE'))
    expect(component.find('[data-testid="identity-created"]').exists()).toBe(false)
  })

  it('retires an identity only after a confirmation that names it and what it revokes, never the primary', async () => {
    state.identities = two().map(i => i.is_primary ? i : { ...i, usage: { connections: 2, tokens: 1 } })
    const component = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    expect(component.find(`[data-identity="${PRIMARY}"] [data-testid="identity-retire"]`).exists()).toBe(false)

    await component.find(`[data-identity="${BRAND}"] [data-testid="identity-retire"]`).trigger('click')
    const confirm = component.find('[data-testid="identity-retire-confirm"]')
    expect(confirm.text()).toContain('Retire Brand B?')
    expect(confirm.text()).toContain('2 connected apps and 1 unused connection token')
    expect(confirm.text()).toContain('destroys its key')
    expect(state.calls).toEqual([])

    await confirm.findAll('button').find(b => b.text() === 'Retire Brand B')!.trigger('click')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'DELETE', path: BRAND }]))
    await vi.waitFor(() => expect(component.find('[data-testid="identity-retired"]').text()).toContain('Retired Brand B'))
  })

  it('grants and removes a member\'s identities on their team page, only with several', async () => {
    state.identities = two()
    const component = await mount(inApp(IdentityAccess, { member: SAM, administrator: false }), { attachTo: document.body })
    const brand = component.find(`[data-identity="${BRAND}"]`)
    const primary = component.find(`[data-identity="${PRIMARY}"]`)
    expect(primary.attributes('data-state') ?? primary.find('[data-state]').attributes('data-state')).toBe('checked')
    await (brand.element.tagName === 'BUTTON' ? brand : brand.find('button')).trigger('click')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'PUT', path: `${BRAND}/${SAM}` }]))

    // With one identity, nothing is shown.
    component.unmount()
    mounted = []
    state.identities = two().slice(0, 1)
    await clearNuxtData()
    const single = await mount(inApp(IdentityAccess, { member: SAM, administrator: false }))
    expect(single.find('[data-testid="identity-access"]').exists()).toBe(false)
  })

  it('offers the identities when adding a member, the primary preselected, and sends them', async () => {
    state.identities = two()
    const component = await mount(inApp(Team), { attachTo: document.body })
    await vi.waitFor(() => expect(component.find('[data-testid="team-member-identities"]').exists()).toBe(true))
    await component.find('#team-member-name').setValue('Pat')
    await component.find('#team-member-pubkey').setValue('npub1pat')
    await component.findAll('button').find(b => b.text().includes('Add Member') || b.text() === 'Add')!.trigger('click')
    await vi.waitFor(() => expect(state.calls.find(c => c.path === 'team')).toBeTruthy())
    expect(state.calls.find(c => c.path === 'team')!.body).toMatchObject({ name: 'Pat', identities: [PRIMARY] })
  })

  it('still names a retired identity in the log, though Config no longer lists it', async () => {
    const rows: LogEntry[] = [{ id: '1', event_id: 'e1', pubkey: 'a'.repeat(64), event_kind: 1, timestamp: '2025-01-01T10:00:00Z', member_pubkey: SAM, member_name: 'Sam', connection_id: 'c1', identity_pubkey: BRAND }]
    state.identities = two().map(i => i.is_primary ? i : { ...i, retired_at: '2026-11-26T10:00:00Z', available: false })
    const log = await mount(ActivityLog, { props: { rows } })
    await vi.waitFor(() => expect(log.find('[data-testid="log-identity"]').text()).toBe('Brand B'))
    const section = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    expect(section.findAll('[data-identity]').map(r => r.attributes('data-identity'))).toEqual([PRIMARY])
  })

  it('names the identity in the log only once there are several', async () => {
    const rows: LogEntry[] = [{ id: '1', event_id: 'e1', pubkey: 'a'.repeat(64), event_kind: 1, timestamp: '2025-01-01T10:00:00Z', member_pubkey: SAM, member_name: 'Sam', connection_id: 'c1', identity_pubkey: BRAND }]
    state.identities = two()
    const several = await mount(ActivityLog, { props: { rows } })
    await vi.waitFor(() => expect(several.find('[data-testid="log-identity"]').text()).toBe('Brand B'))

    several.unmount()
    mounted = []
    state.identities = two().slice(0, 1)
    await clearNuxtData()
    const one = await mount(ActivityLog, { props: { rows } })
    expect(one.find('[data-testid="log-identity"]').exists()).toBe(false)
  })
})
